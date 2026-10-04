import AVFoundation
import AudioToolbox
import QuartzCore
import UIKit
import os

/// How a key sound is actually produced.
enum PlaybackPath: String {
    case engine = "engine"              // AVAudioEngine + preloaded PCM buffers (custom pack sounds)
    case inputClick = "input-click"     // UIDevice.playInputClick (system click, not custom)
    case systemSound = "system-sound"   // AudioServicesPlaySystemSound with the pack's WAVs
    case none = "none"
}

/// Which path the user forced from the status strip (for A/B listening).
enum PathMode: String, CaseIterable {
    case auto, engine, inputClick = "input-click", systemSound = "system-sound"

    var next: PathMode {
        let all = PathMode.allCases
        return all[(all.firstIndex(of: self)! + 1) % all.count]
    }
}

/// Low-latency key sound player for the keyboard extension.
///
/// Startup: configure an .ambient session with a ~5 ms IO buffer, decode every sample the
/// pack references into an AVAudioPCMBuffer in the hardware format, start an AVAudioEngine
/// with 8 player nodes that are already "playing" (idle), so a keystroke is just
/// `scheduleBuffer` on the next voice. If the engine can't start, fall back to
/// playInputClick, and keep SystemSoundIDs for the pack WAVs as a second fallback.
final class SoundEngine {
    static let log = Logger(subsystem: "tech.taktak.ios.keyboard", category: "audio")
    private var log: Logger { SoundEngine.log }

    private static let voiceCount = 8

    private let engine = AVAudioEngine()
    private var players: [AVAudioPlayerNode] = []
    private var nextVoice = 0
    private var buffers: [String: AVAudioPCMBuffer] = [:]
    private var systemSounds: [String: SystemSoundID] = [:]

    private(set) var pack: Pack?
    private(set) var started = false
    private(set) var engineOK = false
    private(set) var engineError: String?
    private(set) var renderClockAdvancing: Bool?
    private(set) var systemSoundsOK = 0
    var mode: PathMode = .auto

    // Latest per-keystroke timings (ms) for the status strip.
    private(set) var lastDispatchMs: Double = 0
    private(set) var lastScheduleMs: Double = 0
    private(set) var maxScheduleMs: Double = 0
    private(set) var keystrokes = 0

    /// The path a keystroke will use right now.
    var activePath: PlaybackPath {
        switch mode {
        case .auto: return engineOK ? .engine : .inputClick
        case .engine: return engineOK ? .engine : .none
        case .inputClick: return .inputClick
        case .systemSound: return systemSoundsOK > 0 ? .systemSound : .none
        }
    }

    // MARK: - Startup

    func start(hasFullAccess: Bool) {
        guard !started else {
            // Returning to the keyboard: make sure the engine is still running.
            if engineOK && !engine.isRunning { restartEngine(reason: "reappear") }
            return
        }
        started = true
        let t0 = CACurrentMediaTime()
        log.notice("startup begin; hasFullAccess=\(hasFullAccess, privacy: .public)")

        configureSession()
        loadPack()
        engineOK = startEngine()
        registerSystemSounds()
        observeNotifications()

        let session = AVAudioSession.sharedInstance()
        log.notice("""
            startup done in \(String(format: "%.1f", (CACurrentMediaTime() - t0) * 1000), privacy: .public) ms; \
            engine=\(self.engineOK ? "ok" : "FAILED", privacy: .public) \
            path=\(self.activePath.rawValue, privacy: .public) \
            outputLatency_ms=\(String(format: "%.2f", session.outputLatency * 1000), privacy: .public) \
            ioBufferDuration_ms=\(String(format: "%.2f", session.ioBufferDuration * 1000), privacy: .public) \
            sampleRate=\(session.sampleRate, privacy: .public) \
            buffers=\(self.buffers.count, privacy: .public) systemSounds=\(self.systemSoundsOK, privacy: .public)
            """)

        // Is the hardware actually pulling audio? A started engine whose render clock
        // never advances would be silent.
        if engineOK {
            let before = engine.outputNode.lastRenderTime?.sampleTime
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { [weak self] in
                guard let self else { return }
                let after = self.engine.outputNode.lastRenderTime?.sampleTime
                let advancing: Bool
                if let b = before, let a = after { advancing = a > b } else { advancing = after != nil }
                self.renderClockAdvancing = advancing
                self.log.notice("engine render clock advancing=\(advancing, privacy: .public) isRunning=\(self.engine.isRunning, privacy: .public) (before=\(before ?? -1, privacy: .public) after=\(after ?? -1, privacy: .public))")
                self.onStatusChange?()
            }
        }
    }

    var onStatusChange: (() -> Void)?

    private func configureSession() {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(.ambient, mode: .default, options: [.mixWithOthers])
            log.notice("session setCategory(.ambient, mixWithOthers) ok")
        } catch {
            log.error("session setCategory failed: \(String(describing: error), privacy: .public)")
        }
        do {
            try session.setPreferredIOBufferDuration(0.005)
            log.notice("session setPreferredIOBufferDuration(0.005) ok")
        } catch {
            log.error("session setPreferredIOBufferDuration failed: \(String(describing: error), privacy: .public)")
        }
        do {
            try session.setActive(true)
            log.notice("session setActive(true) ok")
        } catch {
            log.error("session setActive failed: \(String(describing: error), privacy: .public)")
        }
        log.notice("""
            session after config: category=\(session.category.rawValue, privacy: .public) \
            outputLatency_ms=\(String(format: "%.2f", session.outputLatency * 1000), privacy: .public) \
            ioBufferDuration_ms=\(String(format: "%.2f", session.ioBufferDuration * 1000), privacy: .public) \
            preferredIOBufferDuration_ms=\(String(format: "%.2f", session.preferredIOBufferDuration * 1000), privacy: .public) \
            sampleRate=\(session.sampleRate, privacy: .public) \
            otherAudioPlaying=\(session.isOtherAudioPlaying, privacy: .public)
            """)
    }

    private func loadPack() {
        do {
            let (pack, dir) = try Pack.loadBundled(from: Bundle(for: SoundEngine.self))
            self.pack = pack
            log.notice("pack loaded: id=\(pack.id, privacy: .public) license=\(pack.license, privacy: .public) samples=\(pack.allSamplePaths.count, privacy: .public)")
            for rel in pack.allSamplePaths {
                let url = dir.appendingPathComponent(rel)
                do {
                    let file = try AVAudioFile(forReading: url)
                    guard let buf = AVAudioPCMBuffer(pcmFormat: file.processingFormat,
                                                     frameCapacity: AVAudioFrameCount(file.length)) else { continue }
                    try file.read(into: buf)
                    buffers[rel] = buf  // converted to the engine format in startEngine()
                } catch {
                    log.error("decode failed for a pack sample: \(String(describing: error), privacy: .public)")
                }
            }
        } catch {
            log.error("pack load failed: \(String(describing: error), privacy: .public)")
        }
    }

    private func startEngine() -> Bool {
        // Guard against an output node with no usable format: connecting with an invalid
        // format raises an Objective-C exception that would crash the extension.
        let hw = engine.outputNode.outputFormat(forBus: 0)
        log.notice("engine output node format: sr=\(hw.sampleRate, privacy: .public) ch=\(hw.channelCount, privacy: .public)")
        guard hw.sampleRate > 0, hw.channelCount > 0 else {
            engineError = "output node has no format (sr=\(hw.sampleRate), ch=\(hw.channelCount))"
            log.error("engine start FAILED: \(self.engineError!, privacy: .public)")
            return false
        }
        guard let voiceFormat = AVAudioFormat(standardFormatWithSampleRate: hw.sampleRate, channels: 1) else {
            engineError = "could not build voice format"
            log.error("engine start FAILED: \(self.engineError!, privacy: .public)")
            return false
        }

        // Resample/convert every buffer to the voice format once, up front.
        for (rel, buf) in buffers {
            if let converted = SoundEngine.convert(buf, to: voiceFormat) {
                buffers[rel] = converted
            } else {
                buffers[rel] = nil
                log.error("buffer conversion failed for a pack sample")
            }
        }

        for _ in 0..<SoundEngine.voiceCount {
            let p = AVAudioPlayerNode()
            engine.attach(p)
            engine.connect(p, to: engine.mainMixerNode, format: voiceFormat)
            players.append(p)
        }
        engine.mainMixerNode.outputVolume = pack?.volume ?? 1.0
        engine.prepare()
        do {
            try engine.start()
        } catch {
            engineError = String(describing: error)
            log.error("engine start FAILED: \(String(describing: error), privacy: .public)")
            return false
        }
        players.forEach { $0.play() }  // idle-playing: scheduling a buffer starts it on the next render cycle
        log.notice("engine start OK: voices=\(SoundEngine.voiceCount, privacy: .public) voiceSampleRate=\(voiceFormat.sampleRate, privacy: .public)")
        return true
    }

    private func restartEngine(reason: String) {
        do {
            try engine.start()
            players.forEach { $0.play() }
            log.notice("engine restarted (\(reason, privacy: .public))")
        } catch {
            log.error("engine restart failed (\(reason, privacy: .public)): \(String(describing: error), privacy: .public)")
        }
    }

    private func registerSystemSounds() {
        guard let pack, let base = Bundle(for: SoundEngine.self).url(forResource: "pack", withExtension: nil) else { return }
        var lastStatus: OSStatus = noErr
        for rel in pack.allSamplePaths {
            var sid: SystemSoundID = 0
            let status = AudioServicesCreateSystemSoundID(base.appendingPathComponent(rel) as CFURL, &sid)
            if status == kAudioServicesNoError {
                systemSounds[rel] = sid
            } else {
                lastStatus = status
            }
        }
        systemSoundsOK = systemSounds.count
        log.notice("system sounds registered: \(self.systemSounds.count, privacy: .public)/\(pack.allSamplePaths.count, privacy: .public) lastError=\(lastStatus, privacy: .public)")
    }

    private func observeNotifications() {
        let nc = NotificationCenter.default
        nc.addObserver(forName: .AVAudioEngineConfigurationChange, object: engine, queue: .main) { [weak self] _ in
            guard let self, self.engineOK else { return }
            self.log.notice("engine configuration change")
            self.restartEngine(reason: "configuration change")
        }
        nc.addObserver(forName: AVAudioSession.interruptionNotification, object: nil, queue: .main) { [weak self] note in
            guard let self else { return }
            let raw = note.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt ?? 0
            let type = AVAudioSession.InterruptionType(rawValue: raw)
            self.log.notice("session interruption type=\(raw, privacy: .public)")
            if type == .ended, self.engineOK, !self.engine.isRunning {
                self.restartEngine(reason: "interruption ended")
            }
        }
    }

    func stop() {
        guard engineOK else { return }
        engine.pause()
        log.notice("engine paused (keyboard hidden)")
    }

    // MARK: - Playback

    /// Plays the sample for `key`. `eventTimestamp` is the UIEvent timestamp (seconds since
    /// boot, same clock as CACurrentMediaTime) so we can see input-dispatch delay too.
    /// Never logs the key.
    func play(_ key: KeySound, phase: KeyPhase, eventTimestamp: TimeInterval?) {
        let t0 = CACurrentMediaTime()
        let path = activePath
        var voice = -1

        switch path {
        case .engine:
            if let pack, let rel = key.samplePath(in: pack, phase: phase), let buf = buffers[rel] {
                voice = nextVoice
                nextVoice = (nextVoice + 1) % players.count
                let p = players[voice]
                p.scheduleBuffer(buf, at: nil, options: .interrupts, completionHandler: nil)
                if !p.isPlaying { p.play() }
            }
        case .inputClick:
            if phase == .press { UIDevice.current.playInputClick() }
        case .systemSound:
            if let pack, let rel = key.samplePath(in: pack, phase: phase), let sid = systemSounds[rel] {
                AudioServicesPlaySystemSound(sid)
            }
        case .none:
            break
        }

        let t1 = CACurrentMediaTime()
        let scheduleMs = (t1 - t0) * 1000
        let dispatchMs = eventTimestamp.map { (t0 - $0) * 1000 } ?? -1
        lastScheduleMs = scheduleMs
        lastDispatchMs = dispatchMs
        maxScheduleMs = max(maxScheduleMs, scheduleMs)
        keystrokes += 1
        log.notice("""
            keystroke phase=\(phase.rawValue, privacy: .public) path=\(path.rawValue, privacy: .public) \
            voice=\(voice, privacy: .public) \
            event_to_handler_ms=\(String(format: "%.2f", dispatchMs), privacy: .public) \
            handler_to_scheduled_ms=\(String(format: "%.3f", scheduleMs), privacy: .public)
            """)
    }

    // MARK: - Status

    var statusText: String {
        let session = AVAudioSession.sharedInstance()
        let engineMark: String
        if engineOK {
            engineMark = renderClockAdvancing == false ? "engine ✓ (no render!)" : "engine ✓"
        } else {
            engineMark = "engine ✗"
        }
        var parts = [
            engineMark,
            String(format: "out %.0f ms", session.outputLatency * 1000),
            String(format: "io %.1f ms", session.ioBufferDuration * 1000),
            "path \(activePath.rawValue)\(mode == .auto ? "" : "*")",
        ]
        if keystrokes > 0 {
            parts.append(String(format: "sched %.2f ms", lastScheduleMs))
            if lastDispatchMs >= 0 { parts.append(String(format: "disp %.1f ms", lastDispatchMs)) }
        }
        return parts.joined(separator: " · ")
    }

    // MARK: - Helpers

    static func convert(_ src: AVAudioPCMBuffer, to fmt: AVAudioFormat) -> AVAudioPCMBuffer? {
        if src.format == fmt { return src }
        guard let conv = AVAudioConverter(from: src.format, to: fmt) else { return nil }
        let ratio = fmt.sampleRate / src.format.sampleRate
        let capacity = AVAudioFrameCount(Double(src.frameLength) * ratio) + 1024
        guard let out = AVAudioPCMBuffer(pcmFormat: fmt, frameCapacity: capacity) else { return nil }
        var fed = false
        var error: NSError?
        let status = conv.convert(to: out, error: &error) { _, inStatus in
            if fed {
                inStatus.pointee = .endOfStream
                return nil
            }
            fed = true
            inStatus.pointee = .haveData
            return src
        }
        if status == .error || out.frameLength == 0 { return nil }
        return out
    }
}
