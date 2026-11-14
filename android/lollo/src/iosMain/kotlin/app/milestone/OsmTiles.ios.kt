package app.milestone

import kotlinx.cinterop.CValue
import kotlinx.cinterop.ExperimentalForeignApi
import kotlinx.cinterop.addressOf
import kotlinx.cinterop.cValue
import kotlinx.cinterop.useContents
import kotlinx.cinterop.usePinned
import kotlin.math.floor
import kotlin.math.ln
import kotlin.math.roundToInt
import platform.CoreGraphics.CGBitmapContextCreate
import platform.CoreGraphics.CGBitmapContextCreateImage
import platform.CoreGraphics.CGColorSpaceCreateDeviceRGB
import platform.CoreGraphics.CGColorSpaceRelease
import platform.CoreGraphics.CGContextDrawImage
import platform.CoreGraphics.CGContextRef
import platform.CoreGraphics.CGContextRelease
import platform.CoreGraphics.CGImageAlphaInfo
import platform.CoreGraphics.CGImageGetHeight
import platform.CoreGraphics.CGImageGetWidth
import platform.CoreGraphics.CGImageRelease
import platform.CoreGraphics.CGRectMake
import platform.CoreGraphics.CGSizeMake
import platform.Foundation.NSBundle
import platform.Foundation.NSCache
import platform.Foundation.NSCachesDirectory
import platform.Foundation.NSDate
import platform.Foundation.NSLock
import platform.Foundation.NSSearchPathForDirectoriesInDomains
import platform.Foundation.NSURL
import platform.Foundation.NSURLCache
import platform.Foundation.NSURLRequestUseProtocolCachePolicy
import platform.Foundation.NSURLSession
import platform.Foundation.NSURLSessionConfiguration
import platform.Foundation.NSHTTPURLResponse
import platform.Foundation.NSUserDomainMask
import platform.Foundation.dataTaskWithURL
import platform.Foundation.timeIntervalSince1970
import platform.MapKit.MKMapRect
import platform.MapKit.MKMapSizeWorld
import platform.MapKit.MKOverlayRenderer
import platform.MapKit.MKTileOverlay
import platform.MapKit.MKZoomScale
import platform.UIKit.UIGraphicsPopContext
import platform.UIKit.UIGraphicsPushContext
import platform.UIKit.UIImage

/** The tile cache's size on disk; the system may purge it. */
private const val TILE_DISK_CACHE_BYTES = 134_217_728uL
/** The HTTP layer's in-memory share; decoded tiles live in [OsmTileOverlay]'s cache. */
private const val TILE_MEMORY_CACHE_BYTES = 8_388_608uL
private const val DECODED_TILE_LIMIT = 256L
/** A failed tile is drawn empty and asked for again only after this long (seconds). */
private const val TILE_RETRY_SEC = 60.0
/** Edge of one OSM tile, in pixels and in the points MapKit draws it at. */
private const val TILE_SIZE = 256.0

/** Width (and height) of the MapKit world square, in map points: the span of zoom 0's one tile. */
@OptIn(ExperimentalForeignApi::class)
internal val MapWorldPoints: Double = MKMapSizeWorld.width

/**
 * The one HTTP session every OSM tile request goes through, per the tile usage
 * policy (MapTiles.kt): the bundle identifier as User-Agent (Android sends its
 * package name), at most [OSM_MAX_CONNECTIONS] connections, and a disk cache that
 * honours the server's caching headers, so a tile is fetched again only once it
 * expires. Tiles are fetched only for what a map shows: nothing is prefetched.
 */
@OptIn(ExperimentalForeignApi::class)
private val tileSession: NSURLSession by lazy {
    val config = NSURLSessionConfiguration.defaultSessionConfiguration
    config.HTTPMaximumConnectionsPerHost = OSM_MAX_CONNECTIONS.toLong()
    config.HTTPAdditionalHeaders = mapOf<Any?, Any?>("User-Agent" to (NSBundle.mainBundle.bundleIdentifier ?: "app.milestone"))
    config.requestCachePolicy = NSURLRequestUseProtocolCachePolicy
    val caches = NSSearchPathForDirectoriesInDomains(NSCachesDirectory, NSUserDomainMask, true).firstOrNull() as? String
    config.URLCache = NSURLCache(
        memoryCapacity = TILE_MEMORY_CACHE_BYTES,
        diskCapacity = TILE_DISK_CACHE_BYTES,
        directoryURL = caches?.let { NSURL.fileURLWithPath("$it/osm-tiles", isDirectory = true) },
    )
    NSURLSession.sessionWithConfiguration(config)
}

private fun tileUrl(z: Int, x: Int, y: Int): NSURL? = NSURL.URLWithString(
    OSM_TILE_URL_TEMPLATE.replace("{z}", z.toString()).replace("{x}", x.toString()).replace("{y}", y.toString()),
)

/**
 * OSM's standard tiles as a MapKit overlay that replaces Apple's map content, zoom
 * 0..[OSM_MAX_ZOOM] (deeper views draw the deepest tiles larger). [OsmTileRenderer]
 * loads them through [tileSession] (MKTileOverlay's own loader can't be given the
 * policy's User-Agent, and Kotlin can't override its loading category). With [dark]
 * each tile is desaturated then inverted ([darkTileMatrix], the filter Android's
 * osmdroid maps apply), so the map reads as a dark ground in a dark theme.
 */
@OptIn(ExperimentalForeignApi::class)
internal class OsmTileOverlay(val dark: Boolean) : MKTileOverlay(OSM_TILE_URL_TEMPLATE) {
    private val decoded = NSCache().apply { countLimit = DECODED_TILE_LIMIT.toULong() }
    private val lock = NSLock()
    private val loading = HashSet<String>()
    private val failedAt = HashMap<String, Double>()

    init {
        setCanReplaceMapContent(true)
        minimumZ = 0
        maximumZ = OSM_MAX_ZOOM.toLong()
        tileSize = CGSizeMake(TILE_SIZE, TILE_SIZE)
    }

    private fun key(z: Int, x: Int, y: Int) = "$z/$x/$y"

    fun tile(z: Int, x: Int, y: Int): UIImage? = decoded.objectForKey(key(z, x, y)) as? UIImage

    fun ready(z: Int, x: Int, y: Int): Boolean {
        if (tile(z, x, y) != null) return true
        val k = key(z, x, y)
        lock.lock()
        val failed = failedAt[k]?.let { NSDate().timeIntervalSince1970 - it < TILE_RETRY_SEC } == true
        lock.unlock()
        return failed
    }

    /** Fetches the tile unless it is already on its way; [done] runs once it can be drawn. */
    fun load(z: Int, x: Int, y: Int, done: () -> Unit) {
        val k = key(z, x, y)
        lock.lock()
        val started = loading.add(k)
        lock.unlock()
        if (!started) return
        val url = tileUrl(z, x, y)
        if (url == null) {
            finish(k, null)
            return
        }
        tileSession.dataTaskWithURL(url) { data, response, error ->
            val status = (response as? NSHTTPURLResponse)?.statusCode ?: 0L
            val png = if (error == null && status == 200L) data else null
            val image = png?.let { if (dark) darkTile(it) else UIImage.imageWithData(it) }
            finish(k, image)
            done()
        }.resume()
    }

    private fun finish(k: String, image: UIImage?) {
        if (image != null) decoded.setObject(image, forKey = k)
        lock.lock()
        loading.remove(k)
        if (image == null) failedAt[k] = NSDate().timeIntervalSince1970 else failedAt.remove(k)
        lock.unlock()
    }
}

/**
 * Draws an [OsmTileOverlay]: for each map rect MapKit asks for, the OSM tiles at the
 * zoom that draws them 256 points wide, loaded on demand; the rect is redrawn as
 * each arrives. MapKit calls it from its drawing threads.
 */
@OptIn(ExperimentalForeignApi::class)
internal class OsmTileRenderer(private val tiles: OsmTileOverlay) : MKOverlayRenderer(overlay = tiles) {
    override fun canDrawMapRect(mapRect: CValue<MKMapRect>, zoomScale: MKZoomScale): Boolean {
        var ready = true
        forEachTile(mapRect, zoomScale) { z, x, y, tx, ty, tw ->
            if (!tiles.ready(z, x, y)) {
                ready = false
                tiles.load(z, x, y) {
                    setNeedsDisplayInMapRect(
                        cValue<MKMapRect> {
                            origin.x = tx
                            origin.y = ty
                            size.width = tw
                            size.height = tw
                        },
                        zoomScale,
                    )
                }
            }
        }
        return ready
    }

    override fun drawMapRect(mapRect: CValue<MKMapRect>, zoomScale: MKZoomScale, inContext: CGContextRef?) {
        if (inContext == null) return
        UIGraphicsPushContext(inContext)
        forEachTile(mapRect, zoomScale) { z, x, y, tx, ty, tw ->
            val image = tiles.tile(z, x, y) ?: return@forEachTile
            val rect = rectForMapRect(
                cValue<MKMapRect> {
                    origin.x = tx
                    origin.y = ty
                    size.width = tw
                    size.height = tw
                },
            )
            image.drawInRect(rect)
        }
        UIGraphicsPopContext()
    }

    /**
     * Each tile covering [mapRect] at the zoom whose 256 px tiles show 256 points wide
     * at [zoomScale] (capped at [OSM_MAX_ZOOM]): its zoom, column (wrapped across the
     * antimeridian), row, and its square's origin and edge in map points.
     */
    private inline fun forEachTile(
        mapRect: CValue<MKMapRect>,
        zoomScale: MKZoomScale,
        block: (z: Int, x: Int, y: Int, tx: Double, ty: Double, tw: Double) -> Unit,
    ) {
        val z = (ln(MapWorldPoints * zoomScale / TILE_SIZE) / ln(2.0)).roundToInt().coerceIn(0, OSM_MAX_ZOOM)
        val n = 1 shl z
        val tw = MapWorldPoints / n
        val minX = mapRect.useContents { origin.x }
        val minY = mapRect.useContents { origin.y }
        val maxX = mapRect.useContents { origin.x + size.width }
        val maxY = mapRect.useContents { origin.y + size.height }
        val x0 = floor(minX / tw).toInt()
        val x1 = floor((maxX - 1e-6) / tw).toInt()
        val y0 = floor(minY / tw).toInt().coerceAtLeast(0)
        val y1 = floor((maxY - 1e-6) / tw).toInt().coerceAtMost(n - 1)
        for (ty in y0..y1) for (tx in x0..x1) {
            block(z, tx.mod(n), ty, tx * tw, ty * tw, tw)
        }
    }
}

private val DarkMatrix = darkTileMatrix()

@OptIn(ExperimentalForeignApi::class)
private fun darkTile(png: platform.Foundation.NSData): UIImage? {
    val image = UIImage.imageWithData(png)?.CGImage ?: return null
    val w = CGImageGetWidth(image).toInt()
    val h = CGImageGetHeight(image).toInt()
    if (w <= 0 || h <= 0) return null
    val pixels = ByteArray(w * h * 4)
    val out = IntArray(3)
    return pixels.usePinned { pinned ->
        val space = CGColorSpaceCreateDeviceRGB()
        val ctx = CGBitmapContextCreate(
            pinned.addressOf(0), w.toULong(), h.toULong(), 8u, (w * 4).toULong(), space,
            CGImageAlphaInfo.kCGImageAlphaPremultipliedLast.value,
        )
        CGColorSpaceRelease(space)
        if (ctx == null) return@usePinned null
        CGContextDrawImage(ctx, CGRectMake(0.0, 0.0, w.toDouble(), h.toDouble()), image)
        for (p in 0 until w * h) {
            val i = p * 4
            val a = pixels[i + 3].toInt() and 0xFF
            if (a == 0) continue
            // Premultiplied in the bitmap: straighten, filter, premultiply again.
            val r = (pixels[i].toInt() and 0xFF) * 255 / a
            val g = (pixels[i + 1].toInt() and 0xFF) * 255 / a
            val b = (pixels[i + 2].toInt() and 0xFF) * 255 / a
            applyColorMatrix(DarkMatrix, r, g, b, out)
            pixels[i] = (out[0] * a / 255).toByte()
            pixels[i + 1] = (out[1] * a / 255).toByte()
            pixels[i + 2] = (out[2] * a / 255).toByte()
        }
        val filtered = CGBitmapContextCreateImage(ctx)
        CGContextRelease(ctx)
        if (filtered == null) return@usePinned null
        val result = UIImage.imageWithCGImage(filtered)
        CGImageRelease(filtered)
        result
    }
}
