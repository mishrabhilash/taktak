import Foundation

/// The subset of TakTak's pack.json (format 1) that the spike needs.
struct Pack: Decodable {
    struct Group: Decodable {
        let press: [String]
        let release: [String]?
    }

    let format: Int
    let id: String
    let name: String
    let license: String
    let attribution: String?
    let volume: Float?
    let groups: [String: Group]

    /// Every sample path referenced by any group, deduplicated, in a stable order.
    var allSamplePaths: [String] {
        var seen = Set<String>()
        var out: [String] = []
        for key in groups.keys.sorted() {
            let g = groups[key]!
            for p in g.press + (g.release ?? []) where seen.insert(p).inserted {
                out.append(p)
            }
        }
        return out
    }

    /// Loads `<bundle>/pack/pack.json` (copied there by scripts/copy-pack.sh).
    static func loadBundled(from bundle: Bundle) throws -> (Pack, URL) {
        guard let url = bundle.url(forResource: "pack", withExtension: "json", subdirectory: "pack") else {
            throw NSError(domain: "TakTak", code: 1, userInfo: [NSLocalizedDescriptionKey: "pack/pack.json missing from extension bundle"])
        }
        let pack = try JSONDecoder().decode(Pack.self, from: Data(contentsOf: url))
        return (pack, url.deletingLastPathComponent())
    }
}

enum KeyPhase: String {
    case press, release
}

/// Which sound a key makes. `group` is a pack group name ("alphanumeric", "enter",
/// "backspace", "space", ...); `label` feeds the stable hash so the same key always
/// picks the same sample.
struct KeySound {
    let group: String
    let label: String

    /// The sample for this key and phase. Falls back to the alphanumeric group when the
    /// pack has no dedicated group (tactile has no "space" or "shift" group).
    /// Press and release use the same index so a key's down/up samples stay paired.
    func samplePath(in pack: Pack, phase: KeyPhase) -> String? {
        guard let g = pack.groups[group] ?? pack.groups["alphanumeric"] else { return nil }
        let list: [String]
        switch phase {
        case .press: list = g.press
        case .release: list = g.release ?? []
        }
        guard !list.isEmpty else { return nil }
        return list[Int(KeySound.fnv1a(label) % UInt32(list.count))]
    }

    /// FNV-1a 32-bit. Swift's `hashValue` is seeded per process, so it can't be used
    /// for a mapping that must be identical across launches.
    static func fnv1a(_ s: String) -> UInt32 {
        var h: UInt32 = 0x811c9dc5
        for b in s.utf8 {
            h ^= UInt32(b)
            h = h &* 0x01000193
        }
        return h
    }
}
