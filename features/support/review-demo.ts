// A review with several shots per option, a page, a clip and an option that
// runs live, put on a thread through the mock runner's `mock.review` (as an
// agent's show_options would), for what the demo thread's review hasn't got:
// four options of one picture each. review.feature and tools/review-shot.ts
// use it on both apps.
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const picture = (name: string, height = 1520) => ({ path: path.join(root, 'docs/account-picker-design', `${name}.png`), mediaType: 'image/png', width: 2200, height });

export const SHOTS_REVIEW = 'review_shots';

/** A: three shots; B: two, untitled, and a page; C: one, live at a url;
 * Today: for reference, with shots named as A's, to flick to. */
export function shotsReview() {
  const auto = picture('1-auto-account');
  const picker = picture('2-one-picker');
  const pools = picture('3-pools');
  const moving = picture('4-moving-mid-thread', 800);
  return {
    id: SHOTS_REVIEW,
    title: 'Picker, step by step',
    note: 'Three ways in, each **step by step**.',
    recommended: 'B',
    options: [
      { id: 'A', title: 'Auto account', note: 'Keep the menu, add Auto.', image: auto, shots: [{ title: 'Overview', image: auto }, { title: 'Mid-drag', image: picker }, { title: 'Detail', image: pools }] },
      { id: 'B', title: 'One picker', note: 'One menu, a long one.', image: picker, shots: [{ image: picker }, { image: auto, media: { path: path.join(root, 'docs/account-picker-design/1-auto-account.html'), mediaType: 'text/html' } }] },
      { id: 'C', title: 'Account pools', image: pools, url: 'http://localhost:5190/c', shots: [{ image: pools, media: { path: path.join(root, 'site/public/clips/options.mp4'), mediaType: 'video/mp4' } }] },
      { id: 'Today', title: 'Today', reference: true, image: moving, shots: [{ title: 'Overview', image: moving }, { title: 'Mid-drag', image: auto }] },
    ],
  };
}
