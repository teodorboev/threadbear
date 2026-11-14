// What the thread list's sorting costs with a list the size and shape of Mike's
// (1,400 threads, nearly all archived, over 80 projects, each summary about
// 3.5 KB with its folder's patches, panels, actions and hooks). Not a check:
// it prints times, for docs/gpui-port/perf-hover.md. Run it in release:
//
//   cargo test --release -p threadbear-core --test sidebar_cost -- --ignored --nocapture
use serde_json::{json, Value};
use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;
use threadbear_core::inbox::{self, InboxOrder, ThreadSummary};
use threadbear_core::projects::{Project, Sections};

fn summary(i: usize, project: &str) -> ThreadSummary {
    let open = i < 42;
    let at = format!("2026-11-{:02}T{:02}:{:02}:{:02}.{:03}Z", 1 + i % 12, i % 24, i % 60, (i * 7) % 60, i % 1000);
    let v: Value = json!({
        "id": format!("th_{i}"), "title": format!("{project} task {i}"), "cwd": format!("/Users/mike/dev/me/{project}"),
        "accountId": "claude-main", "provider": "claude", "permissionMode": "full", "status": "idle", "live": false,
        "needsYou": false, "createdAt": at, "updatedAt": at, "lastActivityAt": at, "lastSeq": 40, "lastSeenSeq": 40,
        "preview": "Worked on the thing and it's done now.", "keywords": format!("{project} task build release fix tidy"),
        "archivedAt": if open { Value::Null } else { json!(at) },
        "projects": [{ "id": project, "title": project, "by": "librarian", "at": at }],
        "patches": [
            { "id": "convex", "source": "builtin" },
            { "id": "store", "source": "project", "file": format!("/Users/mike/dev/me/{project}/.threadbear/patches/store.ts"), "icon": "pawprint", "liveError": "node scripts/status.mjs failed (exit 1)" },
            { "id": "checks", "source": "project", "file": format!("/Users/mike/dev/me/{project}/.threadbear/patches/checks.ts"), "icon": "pawprint" },
            { "id": "worktree", "source": "builtin" }
        ],
        "panels": [
            { "id": "convex.dashboard", "title": "Convex", "kind": "url", "url": "https://dashboard.convex.dev", "icon": "cylinder.split.1x2", "patch": "convex", "note": "No deployment in .env.local yet" },
            { "id": "checks.worktrees", "title": "Worktrees", "kind": "list", "patch": "checks", "icon": "arrow.triangle.branch" },
            { "id": "store.status", "title": "Store status", "kind": "url", "url": format!("https://example.com/{project}/status"), "icon": "app.badge", "patch": "store", "summary": "Waiting for review since Tuesday" }
        ],
        "actions": [
            { "id": "store.website", "title": "Website", "kind": "url", "patch": "store", "url": format!("https://example.com/{project}/"), "icon": "safari" },
            { "id": "store.console", "title": "Console", "kind": "url", "patch": "store", "url": format!("https://example.com/{project}/console"), "icon": "app.badge" },
            { "id": "worktree.land", "title": "Land", "kind": "command", "command": "tb land", "patch": "worktree", "icon": "arrow.down.to.line" }
        ],
        "hooks": [{ "id": "checks.typecheck", "title": "Typecheck", "on": "turn.end", "command": "npx tsc --noEmit -p .", "tell": "failures", "patch": "checks", "edited": "src/**/*.ts" }],
        "startedBy": { "kind": "thread", "threadId": format!("th_{}", i + 1), "title": "Another task" }
    });
    serde_json::from_value(v).expect("a summary")
}

fn time<T>(what: &str, rounds: u32, mut f: impl FnMut() -> T) -> f64 {
    let start = Instant::now();
    for _ in 0..rounds {
        black_box(f());
    }
    let ms = start.elapsed().as_secs_f64() * 1000. / f64::from(rounds);
    println!("{what}: {ms:.3} ms");
    ms
}

#[test]
#[ignore = "prints timings; run by hand in release"]
fn sidebar_cost() {
    let projects: Vec<Project> = (0..80)
        .map(|n| Project::decode(&json!({ "id": format!("p{n}"), "title": format!("p{n}"), "folders": [], "createdAt": "2026-10-01T00:00:00Z", "updatedAt": "2026-10-01T00:00:00Z" })).expect("a project"))
        .collect();
    let threads: HashMap<String, ThreadSummary> = (0..1400).map(|i| summary(i, &format!("p{}", i % 80))).map(|t| (t.id.clone(), t)).collect();
    let now = inbox::parse_ms("2026-11-13T12:00:00Z").unwrap();
    let wakes = HashMap::new();
    println!("1,400 threads, 80 projects");
    let one = time("inbox sections, once (copies every thread)", 200, || inbox::sections_ordered(threads.values(), now, InboxOrder::NeedsYou, &wakes));
    // As the app keeps them (thread_map.rs): shared, so the sections copy
    // pointers. They're worked out again whenever a thread changes.
    let shared: HashMap<String, std::sync::Arc<ThreadSummary>> = threads.iter().map(|(k, t)| (k.clone(), std::sync::Arc::new(t.clone()))).collect();
    time("inbox sections, once, shared rows (copies pointers)", 200, || inbox::sections_ordered(shared.values(), now, InboxOrder::NeedsYou, &wakes));
    println!("  the list, its badge and the arrow keys asked for it 3 to 4 times a frame: {:.2} to {:.2} ms a frame, now 0 unless a thread changed", one * 3., one * 4.);
    time("projects list sections", 200, || Sections::build(&projects, threads.values(), now).top_level().len());
    // The Projects list's sorts, with each comparison reading both threads'
    // times again (as it did) and with each time read once (as now).
    let all: Vec<&ThreadSummary> = threads.values().collect();
    time("sorting every thread, reading times each comparison", 200, || {
        let mut v = all.clone();
        v.sort_by(|a, b| b.activity_ms().cmp(&a.activity_ms()).then_with(|| a.id.cmp(&b.id)));
        v.len()
    });
    time("sorting every thread, reading each time once", 200, || {
        let mut v = all.clone();
        v.sort_by_cached_key(|t| (std::cmp::Reverse(t.activity_ms()), t.id.as_str()));
        v.len()
    });
    time("copy of every thread (what the side pane made twice a draw)", 200, || threads.clone());
}
