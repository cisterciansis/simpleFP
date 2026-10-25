import SwiftUI

@main
struct SimpleFPApp: App {
    @State private var library = Library()

    var body: some Scene {
        Window("SimpleFP", id: "main") {
            ContentView()
                .environment(library)
                .frame(minWidth: 820, minHeight: 600)
        }
        .defaultSize(width: 1040, height: 760)
        .windowToolbarStyle(.unified)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("Open TDT Block…") { library.presentOpenPanel() }
                    .keyboardShortcut("o")
                Button("Open Demo Recording") { Task { await library.openDemo() } }
                    .keyboardShortcut("o", modifiers: [.command, .shift])
            }
            CommandGroup(after: .saveItem) {
                Button("Export Results…") { library.presentExportPanel() }
                    .keyboardShortcut("e")
                    .disabled(library.selected?.results == nil)
            }
        }
    }
}
