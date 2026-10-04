import UIKit
import os

/// Root view of the keyboard. Conforming to UIInputViewAudioFeedback with
/// enableInputClicksWhenVisible = true is what makes UIDevice.playInputClick() audible.
final class KeyboardInputView: UIInputView, UIInputViewAudioFeedback {
    var enableInputClicksWhenVisible: Bool { true }
}

private enum KeyKind {
    case character(String)
    case space, ret, backspace, shift, globe
}

private struct KeyDef {
    let kind: KeyKind
    let widthUnits: CGFloat

    /// Sound selection is by key identity, never by the shifted character, so "a" and "A"
    /// sound the same.
    var sound: KeySound {
        switch kind {
        case .character(let c): return KeySound(group: "alphanumeric", label: c)
        case .space: return KeySound(group: "space", label: "space")
        case .ret: return KeySound(group: "enter", label: "enter")
        case .backspace: return KeySound(group: "backspace", label: "backspace")
        case .shift: return KeySound(group: "shift", label: "shift")
        case .globe: return KeySound(group: "alphanumeric", label: "globe")
        }
    }
}

private final class KeyButton: UIButton {
    let def: KeyDef
    init(def: KeyDef) {
        self.def = def
        super.init(frame: .zero)
    }
    required init?(coder: NSCoder) { fatalError() }
}

final class KeyboardViewController: UIInputViewController {
    private let sound = SoundEngine()
    private let statusLabel = UILabel()
    private var rows: [[KeyButton]] = []
    private var globeButton: KeyButton?
    private var shiftOn = false
    private var heightConstraint: NSLayoutConstraint?

    private static let keyHeight: CGFloat = 46
    private static let rowGap: CGFloat = 10
    private static let keyGap: CGFloat = 6
    private static let statusHeight: CGFloat = 20
    private static let sidePad: CGFloat = 3

    override func loadView() {
        let v = KeyboardInputView(frame: CGRect(x: 0, y: 0, width: 320, height: 260), inputViewStyle: .keyboard)
        v.allowsSelfSizing = true
        view = v
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        buildStatusLabel()
        buildKeys()
        let total = Self.statusHeight + 4 * Self.keyHeight + 4 * Self.rowGap + 4
        let h = view.heightAnchor.constraint(equalToConstant: total)
        h.priority = UILayoutPriority(999)
        h.isActive = true
        heightConstraint = h
        sound.onStatusChange = { [weak self] in self?.refreshStatus() }
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        sound.start(hasFullAccess: hasFullAccess)
        refreshStatus()
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        // hasFullAccess is only reliable once the view is on screen.
        SoundEngine.log.notice("keyboard appeared; hasFullAccess=\(self.hasFullAccess, privacy: .public) needsInputModeSwitchKey=\(self.needsInputModeSwitchKey, privacy: .public) path=\(self.sound.activePath.rawValue, privacy: .public)")
        refreshStatus()
    }

    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        sound.stop()
    }

    override func viewWillLayoutSubviews() {
        super.viewWillLayoutSubviews()
        globeButton?.isHidden = !needsInputModeSwitchKey
        layoutKeys()
    }

    // MARK: - Build

    private func buildStatusLabel() {
        statusLabel.font = .monospacedSystemFont(ofSize: 10, weight: .regular)
        statusLabel.textColor = .secondaryLabel
        statusLabel.textAlignment = .center
        statusLabel.adjustsFontSizeToFitWidth = true
        statusLabel.minimumScaleFactor = 0.6
        statusLabel.isUserInteractionEnabled = true
        statusLabel.addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(cyclePath)))
        statusLabel.accessibilityHint = "Cycles the sound playback path"
        view.addSubview(statusLabel)
    }

    private func buildKeys() {
        let letterRows = ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
        var defs: [[KeyDef]] = letterRows.map { row in row.map { KeyDef(kind: .character(String($0)), widthUnits: 1) } }
        defs[2].insert(KeyDef(kind: .shift, widthUnits: 1.4), at: 0)
        defs[2].append(KeyDef(kind: .backspace, widthUnits: 1.4))
        defs.append([
            KeyDef(kind: .globe, widthUnits: 1.4),
            KeyDef(kind: .space, widthUnits: 5.6),
            KeyDef(kind: .ret, widthUnits: 2.2),
        ])

        rows = defs.map { row in row.map(makeButton) }
        rows.flatMap { $0 }.forEach { view.addSubview($0) }
        updateLetterTitles()
    }

    private func makeButton(_ def: KeyDef) -> KeyButton {
        let b = KeyButton(def: def)
        var config = UIButton.Configuration.filled()
        config.cornerStyle = .medium
        config.baseForegroundColor = .label
        config.contentInsets = .zero
        switch def.kind {
        case .character:
            config.baseBackgroundColor = .systemBackground
        default:
            config.baseBackgroundColor = .systemGray3
        }
        switch def.kind {
        case .space: config.title = "space"
        case .ret: config.title = "return"
        case .backspace: config.image = UIImage(systemName: "delete.left")
        case .shift: config.image = UIImage(systemName: "shift")
        case .globe: config.image = UIImage(systemName: "globe")
        case .character: break
        }
        b.configuration = config
        b.layer.shadowColor = UIColor.black.cgColor
        b.layer.shadowOpacity = 0.25
        b.layer.shadowOffset = CGSize(width: 0, height: 1)
        b.layer.shadowRadius = 0

        if case .globe = def.kind {
            // Apple's recommended wiring: handles tap-to-switch and long-press input mode list.
            b.addTarget(self, action: #selector(handleInputModeList(from:with:)), for: .allTouchEvents)
            b.addTarget(self, action: #selector(keyDown(_:event:)), for: .touchDown)
            globeButton = b
        } else {
            b.addTarget(self, action: #selector(keyDown(_:event:)), for: .touchDown)
            b.addTarget(self, action: #selector(keyUpInside(_:event:)), for: .touchUpInside)
            b.addTarget(self, action: #selector(keyUpOutside(_:event:)), for: [.touchUpOutside, .touchCancel])
        }
        return b
    }

    private func updateLetterTitles() {
        for b in rows.flatMap({ $0 }) {
            switch b.def.kind {
            case .character(let c):
                var config = b.configuration
                var title = AttributedString(shiftOn ? c.uppercased() : c)
                title.font = .systemFont(ofSize: 22)
                config?.attributedTitle = title
                b.configuration = config
            case .shift:
                var config = b.configuration
                config?.image = UIImage(systemName: shiftOn ? "shift.fill" : "shift")
                b.configuration = config
            default:
                break
            }
        }
    }

    private func layoutKeys() {
        let w = view.bounds.width
        guard w > 0 else { return }
        statusLabel.frame = CGRect(x: 4, y: 2, width: w - 8, height: Self.statusHeight)
        let unit = (w - 2 * Self.sidePad - 9 * Self.keyGap) / 10 + Self.keyGap  // width of 1 key incl. its gap
        var y = Self.statusHeight + Self.rowGap / 2
        for row in rows {
            let visible = row.filter { !$0.isHidden }
            var units = visible.reduce(0) { $0 + $1.def.widthUnits }
            // Without a globe key, let space absorb its width.
            let spaceExtra: CGFloat = (row.contains { if case .space = $0.def.kind { return true }; return false } && units < 10) ? 10 - units : 0
            units += spaceExtra
            let rowWidth = units * unit - Self.keyGap
            var x = (w - rowWidth) / 2
            for b in visible {
                var u = b.def.widthUnits
                if case .space = b.def.kind { u += spaceExtra }
                let kw = u * unit - Self.keyGap
                b.frame = CGRect(x: x, y: y, width: kw, height: Self.keyHeight)
                x += kw + Self.keyGap
            }
            y += Self.keyHeight + Self.rowGap
        }
    }

    // MARK: - Touch handling

    @objc private func keyDown(_ sender: KeyButton, event: UIEvent) {
        // Sound first: nothing else happens between the touch and the schedule call.
        let ts = event.allTouches?.first(where: { $0.view === sender })?.timestamp ?? event.timestamp
        sound.play(sender.def.sound, phase: .press, eventTimestamp: ts)
    }

    @objc private func keyUpInside(_ sender: KeyButton, event: UIEvent) {
        let ts = event.allTouches?.first(where: { $0.view === sender })?.timestamp ?? event.timestamp
        sound.play(sender.def.sound, phase: .release, eventTimestamp: ts)
        let proxy = textDocumentProxy
        switch sender.def.kind {
        case .character(let c):
            proxy.insertText(shiftOn ? c.uppercased() : c)
            if shiftOn {
                shiftOn = false
                updateLetterTitles()
            }
        case .space: proxy.insertText(" ")
        case .ret: proxy.insertText("\n")
        case .backspace: proxy.deleteBackward()
        case .shift:
            shiftOn.toggle()
            updateLetterTitles()
        case .globe: break
        }
        refreshStatus()
    }

    @objc private func keyUpOutside(_ sender: KeyButton, event: UIEvent) {
        sound.play(sender.def.sound, phase: .release, eventTimestamp: event.timestamp)
    }

    @objc private func cyclePath() {
        sound.mode = sound.mode.next
        SoundEngine.log.notice("playback mode set to \(self.sound.mode.rawValue, privacy: .public) -> path \(self.sound.activePath.rawValue, privacy: .public)")
        refreshStatus()
    }

    private func refreshStatus() {
        statusLabel.text = sound.statusText + " · full access: " + (hasFullAccess ? "yes" : "no")
    }
}
