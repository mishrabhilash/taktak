import SwiftUI
import UIKit

/// Host app for the keyboard spike: explains how to enable the keyboard and gives
/// a place to type with it. The keyboard itself lives in the TakTakKeyboard target.
struct ContentView: View {
    @State private var line = ""
    @State private var paragraph = ""
    @State private var showNotes = true
    @AppStorage("spikeNotes") private var notes = ""

    var body: some View {
        NavigationStack {
            Form {
                Section("Enable the keyboard") {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("1. Open Settings → General → Keyboard → Keyboards.")
                        Text("2. Tap Add New Keyboard… and pick TakTak.")
                        Text("3. Leave Allow Full Access OFF (this spike tests sound without it; the keyboard does not even request it).")
                        Text("4. Come back here, tap a text field and switch keyboards with the globe key.")
                    }
                    .font(.callout)
                    Button("Open Settings") {
                        if let url = URL(string: UIApplication.openSettingsURLString) {
                            UIApplication.shared.open(url)
                        }
                    }
                }

                Section("Type here") {
                    TextField("Single line", text: $line)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    TextEditor(text: $paragraph)
                        .frame(minHeight: 120)
                }

                Section {
                    Toggle("Show spike notes", isOn: $showNotes)
                    if showNotes {
                        VStack(alignment: .leading, spacing: 6) {
                            Text("The strip above the keys shows the live audio state, e.g. “engine ✓ · out 12 ms · io 5.3 ms · full access: no”.")
                            Text("Tap that strip to cycle the playback path: auto → engine → input click → system sound.")
                            Text("“sched” is the time from touch-down to the buffer being scheduled; “disp” is the time from the touch event's timestamp to our handler running.")
                            Text("Logs: xcrun simctl spawn booted log stream --predicate 'subsystem == \"tech.taktak.ios.keyboard\"'")
                                .font(.caption.monospaced())
                                .textSelection(.enabled)
                            Text("No log line ever says which key was pressed.")
                        }
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        TextEditor(text: $notes)
                            .frame(minHeight: 80)
                            .overlay(alignment: .topLeading) {
                                if notes.isEmpty {
                                    Text("Your observations…")
                                        .foregroundStyle(.tertiary)
                                        .padding(.top, 8)
                                        .padding(.leading, 5)
                                        .allowsHitTesting(false)
                                }
                            }
                    }
                } header: {
                    Text("Spike")
                } footer: {
                    Text("Sounds: Tactile pack (CC0 1.0), recordings by StavSounds, alpinemesh and yottasounds on Freesound.")
                }
            }
            .navigationTitle("TakTak")
        }
    }
}

#Preview {
    ContentView()
}
