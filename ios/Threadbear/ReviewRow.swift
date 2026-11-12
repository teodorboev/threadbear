import SwiftUI
import ThreadbearKit

/// Another lab's review as a quiet row over its turn's reply, as the Mac
/// draws it (desktop/gpui/src/rows/lab_review.rs) and the website does: who
/// reviewed and how it came out, which opens the review's thread, then each
/// finding with what was done about it. The words are LabReviewText's.
struct ReviewRow: View {
    let review: PhoneLabReview
    let openThread: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button {
                openThread(review.threadId)
            } label: {
                Text(LabReviewText.heading(review))
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.leading)
            }
            .buttonStyle(.plain)
            .disabled(review.threadId.isEmpty)
            .accessibilityHint("Opens the review's thread")
            ForEach(review.findings) { finding in
                let (kind, _) = LabReviewText.outcome(finding)
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: symbol(kind))
                        .font(.footnote)
                        .foregroundStyle(color(kind))
                        .frame(width: 16)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(finding.claim)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                        Text(LabReviewText.detail(finding))
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                    }
                }
                .accessibilityElement(children: .combine)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 2)
    }

    // The Mac's marks, in SF Symbols.
    private func symbol(_ kind: LabReviewText.Outcome) -> String {
        switch kind {
        case .fixed: "checkmark.circle"
        case .disputed: "exclamationmark.bubble"
        case .left: "person.crop.circle"
        case .none: "circle"
        }
    }

    private func color(_ kind: LabReviewText.Outcome) -> Color {
        switch kind {
        case .fixed: .green
        case .disputed: .orange
        case .left: .accentColor
        case .none: .secondary
        }
    }
}
