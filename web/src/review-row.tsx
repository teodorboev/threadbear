// Another lab's review as a quiet row over its turn's reply, as the Mac draws
// it (desktop/gpui/src/rows/lab_review.rs): who reviewed and how it came out,
// which opens the review's thread, then each finding with what was done about
// it. The wording is lab-review.ts.
import * as Icon from './icons';
import { go } from './route';
import { findingDetail, outcomeOf, reviewHeading, type LabReviewLedger } from './lab-review';

const MARKS = { fixed: Icon.CircleCheck, disputed: Icon.BubbleWarning, left: Icon.CircleUser, none: Icon.Circle };

export function ReviewRow({ review, threadPath }: { review: LabReviewLedger; threadPath: (id: string) => string }) {
  const heading = reviewHeading(review);
  const path = review.threadId ? threadPath(review.threadId) : undefined;
  return (
    <div className="lab-review">
      {path ? (
        <a
          className="lab-review-head"
          href={path}
          title="Open the review's thread"
          onClick={(e) => {
            if (e.metaKey || e.ctrlKey) return;
            e.preventDefault();
            go(path);
          }}
        >
          {heading}
        </a>
      ) : (
        <div className="lab-review-head">{heading}</div>
      )}
      {review.findings.map((f) => {
        const { kind } = outcomeOf(f);
        const Mark = MARKS[kind];
        return (
          <div key={f.id} className="lab-finding">
            <span className={`lab-mark ${kind}`}>
              <Mark />
            </span>
            <div>
              <div className="lab-claim">{f.claim}</div>
              <div className="lab-detail">{findingDetail(f)}</div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
