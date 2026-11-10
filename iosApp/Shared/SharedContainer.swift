import Foundation

/// The App Group container the app and its widget extension share: the app writes
/// the widget payload there, the extension reads it. Compiled into both targets.
///
/// Both entitlements files ask for LOLLOP_APP_GROUP (iosApp/project.yml), which each
/// bundle's Info.plist carries as `LollopAppGroup`. Signed by Xcode that is the group;
/// SideStore (free Apple Account) registers it as `<group>.<TEAMID>` and signs both
/// bundles with that one (SideStore's FetchProvisioningProfilesOperation.swift
/// appends the team ID), so the identifier is looked up at runtime, in this order:
/// 1. `ALTAppGroups` in Info.plist, which SideStore 0.6 and 0.7 releases write into
///    the app and each extension (ResignAppOperation.swift);
/// 2. the application groups in the bundle's `embedded.mobileprovision`, which every
///    SideStore version writes into each bundle (SideStore nightly no longer writes
///    `ALTAppGroups`);
/// 3. the group as declared.
/// A candidate counts only when the system hands out its container.
enum SharedContainer {
    /// The group as declared: `LollopAppGroup` in the Info.plist of the bundle this
    /// file is compiled into (the app, the extension, or the extension's tests).
    static let declaredGroup: String = {
        guard let group = Bundle(for: BundleMarker.self).object(forInfoDictionaryKey: "LollopAppGroup") as? String,
              !group.isEmpty
        else { preconditionFailure("Info.plist lacks LollopAppGroup (LOLLOP_APP_GROUP in iosApp/project.yml)") }
        return group
    }()

    private final class BundleMarker {}

    /// The widget payload's file name in the container.
    static let payloadFile = "widget-today.json"

    /// The container, or nil when the bundle is entitled to no matching group (an
    /// install that dropped the App Group): the widget then reads "Open lollop to update".
    static let url: URL? = {
        for group in candidateGroups() {
            if let url = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group) {
                return url
            }
        }
        return nil
    }()

    static var payloadURL: URL? { url?.appendingPathComponent(payloadFile) }

    static func candidateGroups(bundle: Bundle = .main) -> [String] {
        var groups: [String] = []
        if let alt = bundle.object(forInfoDictionaryKey: "ALTAppGroups") as? [String] {
            groups += alt.filter { $0.hasPrefix(declaredGroup) }
        }
        groups += provisionedGroups(bundle: bundle).filter { $0.hasPrefix(declaredGroup) }
        groups.append(declaredGroup)
        var seen = Set<String>()
        return groups.filter { seen.insert($0).inserted }
    }

    /// The application groups in the bundle's provisioning profile (a CMS envelope
    /// around a plist), or none.
    static func provisionedGroups(bundle: Bundle = .main) -> [String] {
        let profile = bundle.bundleURL.appendingPathComponent("embedded.mobileprovision")
        guard let data = try? Data(contentsOf: profile) else { return [] }
        return applicationGroups(inProfile: data)
    }

    static func applicationGroups(inProfile data: Data) -> [String] {
        guard let start = data.range(of: Data("<?xml".utf8)),
              let end = data.range(of: Data("</plist>".utf8), in: start.lowerBound..<data.endIndex)
        else { return [] }
        let plistData = data.subdata(in: start.lowerBound..<end.upperBound)
        guard let plist = try? PropertyListSerialization.propertyList(from: plistData, format: nil) as? [String: Any],
              let entitlements = plist["Entitlements"] as? [String: Any],
              let groups = entitlements["com.apple.security.application-groups"] as? [String]
        else { return [] }
        return groups
    }
}
