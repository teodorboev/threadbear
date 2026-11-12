import SwiftUI
import ThreadbearKit

extension EnvironmentValues {
    /// Opens a plan in its sheet (ThreadView sets it), from its card in the
    /// transcript or the card it waits on.
    @Entry var openPlan: (PhoneMessage) -> Void = { _ in }
}

/// A Plan first plan in the transcript (backend API 72): its title and first
/// lines, opening the whole plan. The phone gets the plan in full only from
/// this row, as the card it waits on carries one line.
struct PlanCard: View {
    let message: PhoneMessage
    @Environment(\.openPlan) private var openPlan

    var body: some View {
        Button { openPlan(message) } label: {
            VStack(alignment: .leading, spacing: 8) {
                Label("Plan", systemImage: "list.bullet.clipboard")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                Text(Plans.title(of: message.text))
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .multilineTextAlignment(.leading)
                let preview = Self.preview(Plans.body(of: message.text))
                if !preview.isEmpty {
                    Text(MarkdownText.inline(preview))
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .multilineTextAlignment(.leading)
                        .lineLimit(4)
                }
                Label("Read the plan", systemImage: "chevron.right")
                    .labelStyle(TrailingIcon())
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.accentColor)
            }
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            .contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        }
        .buttonStyle(.plain)
        .accessibilityHint("Opens the whole plan")
    }

    /// The first few lines as plain words: no heading or list marks, which
    /// read as noise squeezed into four lines.
    static func preview(_ body: String) -> String {
        body.components(separatedBy: "\n")
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty && !$0.hasPrefix("```") && !$0.hasPrefix("|") && $0 != "---" }
            .prefix(6)
            .map { line in
                var l = Substring(line)
                while l.first == "#" { l = l.dropFirst() }
                if let marker = l.firstMatch(of: /^\s*([-*+]|\d+[.)])\s+/) { l = l[marker.range.upperBound...] }
                return l.trimmingCharacters(in: .whitespaces)
            }
            .joined(separator: " ")
    }
}

private struct TrailingIcon: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 4) { configuration.title; configuration.icon.imageScale(.small) }
    }
}

/// The whole plan, to read, share or save as a Markdown file, and, while
/// it waits, to approve or send back with what should change.
struct PlanSheet: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    let plan: PhoneMessage
    /// The card this plan waits on when it's the newest and can be allowed
    /// from here; nil for an older plan, a Codex one or one the Mac must allow.
    let approvalId: String?
    /// Whether a message from here reaches the plan: it's the newest one.
    let canSendBack: Bool
    var goAhead: (String) -> Void
    var sendBack: (String) async -> Bool
    @State private var change = ""
    @State private var sending = false
    @State private var file: URL?
    @State private var copied = false
    @State private var approved = false
    @FocusState private var typing: Bool

    private var title: String { Plans.title(of: plan.text) }
    /// Spinning from the tap until the card goes (AppModel.answerCard).
    private var approving: Bool { approvalId.map { model.cardAnswers[$0] != nil } ?? false }
    private var typed: String { change.trimmingCharacters(in: .whitespacesAndNewlines) }

    var body: some View {
        NavigationStack {
            ScrollView {
                MarkdownText(plan.text, seq: plan.seq)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding()
            }
            .scrollDismissesKeyboard(.interactively)
            .navigationTitle(title)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) {
                    Button("Done") { dismiss() }
                }
                ToolbarItemGroup(placement: .topBarTrailing) {
                    Button {
                        UIPasteboard.general.string = plan.text
                        copied = true
                    } label: {
                        Image(systemName: copied ? "checkmark" : "doc.on.doc")
                            .contentTransition(.symbolEffect(.replace))
                    }
                    .accessibilityLabel(copied ? "Copied" : "Copy the plan")
                    .sensoryFeedback(.success, trigger: copied) { _, new in new }
                    // Save to Files, Notes, Mail or AirDrop, as a .md file.
                    if let file {
                        ShareLink(item: file, preview: SharePreview(title)) {
                            Image(systemName: "square.and.arrow.up")
                        }
                        .accessibilityLabel("Share or save the plan")
                    }
                }
            }
            .safeAreaInset(edge: .bottom) {
                if canSendBack { decisionBar }
            }
            .task(id: plan.seq) { file = writeFile() }
            // Approved: the sheet goes once its card has, and the thread is
            // what's next. A refused one stays, with why under the thread.
            .onChange(of: approvalId) { _, now in if approved, now == nil { dismiss() } }
        }
    }

    /// Go ahead with nothing typed; Send back once there's something to change.
    private var decisionBar: some View {
        VStack(spacing: 10) {
            TextField(approvalId == nil ? "Ask for changes" : "What should change?", text: $change, axis: .vertical)
                .lineLimit(1...5)
                .focused($typing)
                .padding(.horizontal, 12)
                .padding(.vertical, 10)
                .background(Color(.tertiarySystemFill), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .disabled(sending || approving)
            if let approvalId, typed.isEmpty {
                Button {
                    approved = true
                    goAhead(approvalId)
                } label: {
                    Text("Go ahead")
                        .frame(maxWidth: .infinity)
                        .spinning(approving, tint: .white)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .allowsHitTesting(!approving)
                .accessibilityLabel(approving ? "Go ahead, sending" : "Go ahead")
            } else {
                Button {
                    send()
                } label: {
                    Text("Send back")
                        .frame(maxWidth: .infinity)
                        .spinning(sending, tint: .white)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                .disabled(typed.isEmpty && !sending)
                .allowsHitTesting(!sending)
            }
        }
        .padding()
        .background(.bar)
        .sensoryFeedback(.impact(weight: .light), trigger: approving) { _, new in new }
    }

    private func send() {
        guard !typed.isEmpty, !sending else { return }
        sending = true
        typing = false
        Task {
            let ok = await sendBack(typed)
            sending = false
            if ok {
                change = ""
                dismiss()
            }
        }
    }

    /// The plan as a Markdown file in the app's temporary folder, for the share sheet.
    private func writeFile() -> URL? {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("plans", isDirectory: true)
        let url = dir.appendingPathComponent(Plans.fileName(for: plan.text))
        do {
            try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
            try Data(plan.text.utf8).write(to: url, options: .atomic)
            return url
        } catch {
            return nil
        }
    }
}
