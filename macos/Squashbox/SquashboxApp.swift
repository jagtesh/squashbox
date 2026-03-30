// Squashbox — Minimal host app for the FSKit extension.
//
// This app exists solely to contain the SquashboxFS.appex extension.
// It shows a brief message directing users to enable the extension
// in System Settings.

import SwiftUI

@main
struct SquashboxApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
        }
    }
}

struct ContentView: View {
    var body: some View {
        VStack(spacing: 20) {
            Image(systemName: "externaldrive.badge.checkmark")
                .font(.system(size: 64))
                .foregroundColor(.accentColor)

            Text("Squashbox")
                .font(.largeTitle)
                .fontWeight(.bold)

            Text("SquashFS Filesystem Extension")
                .font(.title3)
                .foregroundColor(.secondary)

            Divider()
                .frame(width: 200)

            VStack(alignment: .leading, spacing: 8) {
                Label("Enable the extension:", systemImage: "gear")
                    .font(.headline)

                Text("System Settings → General → Login Items & Extensions → File System Extensions")
                    .font(.body)
                    .foregroundColor(.secondary)
                    .padding(.leading, 28)

                Label("Then mount images:", systemImage: "terminal")
                    .font(.headline)
                    .padding(.top, 8)

                Text("sqb mount image.sqsh /Volumes/MyImage")
                    .font(.system(.body, design: .monospaced))
                    .foregroundColor(.secondary)
                    .padding(.leading, 28)
            }
            .padding()

            Button("Open System Settings…") {
                NSWorkspace.shared.open(
                    URL(string: "x-apple.systempreferences:com.apple.LoginItems-Settings.extension")!
                )
            }
            .controlSize(.large)
        }
        .frame(width: 480, height: 420)
        .padding()
    }
}
