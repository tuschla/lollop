/**
 * Prints the ART heap limits of a VM started the way the zygote starts one (via
 * app_process, which reads the same dalvik.vm.heap* properties). Used by
 * scripts/emulator-scenarios.sh to prove a dalvik.vm.heapgrowthlimit override is
 * in force before booting the app under it.
 *
 *   app_process -cp /data/local/tmp/heapprobe.dex /system/bin HeapProbe
 */
public class HeapProbe {
    public static void main(String[] args) {
        long max = Runtime.getRuntime().maxMemory();
        System.out.println("maxMemoryMB=" + (max / (1024 * 1024)));
    }
}
