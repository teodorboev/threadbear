use serde_json::json;
use threadbear_core::{inbox::ThreadSummary, logins::RunnerLogins, remote::decode_remote_threads, sign_in};

#[test]
fn malformed_row_and_empty_list_cannot_remove_threads_or_authorize_cleanup() {
    use threadbear_core::inbox::ThreadList;
    let fixture = serde_json::from_str(include_str!("fixtures/wire-thread-list.json")).unwrap();
    let mut threads = std::collections::HashMap::from([("kept".into(), ThreadSummary { id:"kept".into(), ..Default::default() })]);
    let list = ThreadList::decode(&fixture).expect("skip just the bad rows");
    assert_eq!(list.rows.len(), 1);
    assert!(!list.apply(&mut threads));
    assert!(threads.contains_key("kept"));
    assert!(threads.contains_key("good"));
    assert!(!ThreadList::decode(&json!({"threads":[]})).unwrap().apply(&mut threads));
    assert_eq!(threads.len(), 2);
    for invalid in [json!({}), json!({"threads":null}), json!({"threads":42})] {
        assert!(ThreadList::decode(&invalid).is_err());
    }
    assert!(ThreadList::decode(&json!({"threads":[{"id":"new"}]})).unwrap().apply(&mut threads));
    assert_eq!(threads.len(), 1);
    assert!(threads.contains_key("new"));
}

#[test]
fn nested_account_models_and_defaults_keep_their_valid_siblings() {
    let logins = RunnerLogins::from_hello(&json!({
        "accounts":[{"id":"a", "email":42, "models":[{"id":"m", "description":42, "effortLevels":["high",false]}, 42]}, false, {"id":"b"}],
        "pools":[{"id":"p","accountIds":["a",42,"b"],"icon":false}, false],
        "defaults":{"cwd":"/work","models":{"claude":"m","codex":42},"efforts":false}
    }));
    assert_eq!(logins.accounts.len(), 2);
    assert_eq!(logins.accounts[0].models.len(), 1);
    assert_eq!(logins.accounts[0].models[0].title, "m");
    assert_eq!(logins.accounts[0].models[0].effort_levels, ["high"]);
    assert_eq!(logins.pools[0].account_ids, ["a", "b"]);
    assert_eq!(logins.defaults.cwd, "/work");
    assert_eq!(logins.defaults.model_for("claude"), Some("m"));
    assert_eq!(logins.defaults.model_for("codex"), None);
}

#[test]
fn other_protocol_structs_recover_fields_and_nested_lists() {
    use threadbear_core::{highlights::{ProjectInfo, Schedule, Panel}, convex_panel::{Choices, Key}, reply_buttons::ReplyButton, slash::decode_commands};
    let projects = ProjectInfo::list(&json!([{"id":"p", "title":"Kept project", "archived":{}, "links":[{"title":42,"url":"https://example.com"},false]}, false]));
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].title, "Kept project");
    assert_eq!(projects[0].links.len(), 1);
    let schedules = Schedule::list(&json!([{"id":"s","title":"Kept schedule","runs":[],"lastRun":{"at":42,"outcome":"sent"}},false]));
    assert_eq!(schedules.len(), 1);
    assert_eq!(schedules[0].last_run.as_ref().unwrap().outcome, "sent");
    let panel = Panel::from_value(&json!({"id":"p", "title":"Panel", "badge":[], "convex":{"deployment":"kept","team":42}})).unwrap();
    assert_eq!(panel.convex.unwrap().deployment, "kept");
    let choices: Choices = serde_json::from_value(json!({"deployments":[{"deployment":"kept","label":42},false],"note":42})).unwrap();
    assert_eq!(choices.deployments.len(), 1);
    assert_eq!(choices.deployments[0].deployment, "kept");
    assert!(serde_json::from_value::<Key>(json!({"deployment":"x","deploymentUrl":"https://example.com","adminKey":false})).is_err(), "credentials stay strict");
    let button = ReplyButton::decode(&json!({"id":"b","title":"Button","createdAt":false,"start":{"text":"Kept prompt","cwd":"/work","title":42}})).unwrap();
    assert_eq!(button.start.unwrap().text, "Kept prompt");
    let commands = decode_commands(&json!({"commands":[{"name":"kept","description":42},false]}));
    assert_eq!(commands[0].name, "kept");
}

#[test]
fn browser_tabs_skip_bad_rows_and_keep_other_host_fields() {
    use threadbear_core::browserbear::{agent_state, PanelState};
    let state = agent_state(br#"{"port":123,"socket":"socket","tabs":[{"tabId":1,"url":"https://example.com","title":42},false,{"tabId":2}]}"#).unwrap();
    assert_eq!(state.port, 123);
    assert_eq!(state.tabs.len(), 2);
    assert_eq!(state.tabs[0].url, "https://example.com");
    for tabs in [json!(false),json!({"nested":"wrong"}),json!(42),json!(null),json!(1.2),json!("wrong")] {
        let state = agent_state(serde_json::to_string(&json!({"port":123,"tabs":tabs})).unwrap().as_bytes()).unwrap();
        assert_eq!(state.port, 123);
        assert!(state.tabs.is_empty());
    }
    let panel: PanelState = serde_json::from_value(json!({"tabs":[false,{"tabId":1,"title":42}]})).unwrap();
    assert_eq!(panel.tabs.len(), 1);
    let mut rows = vec![json!(false); threadbear_core::browserbear::MAX_TABS];
    rows.push(json!({"tabId":1}));
    let panel: PanelState = serde_json::from_value(json!({"tabs":rows})).unwrap();
    assert!(panel.tabs.is_empty(), "the cap counts consumed entries, including bad ones");
}

#[test]
fn invalid_identities_and_duplicate_rows_do_not_authorize_cleanup() {
    use threadbear_core::inbox::ThreadList;
    for row in [json!({}),json!({"id":""}),json!({"id":42}),json!(false),json!(null)] {
        assert!(serde_json::from_value::<ThreadSummary>(row).is_err());
    }
    let list = ThreadList::decode(&json!({"threads":[{"id":"same"},{"id":"same"}]})).unwrap();
    let mut threads = std::collections::HashMap::from([("kept".into(), ThreadSummary::default())]);
    assert!(!list.apply(&mut threads));
    assert!(threads.contains_key("kept"));
    let empty = ThreadList::decode(&json!({"threads":[false,{"id":42}]})).unwrap();
    assert!(!empty.apply(&mut threads));
    assert!(threads.contains_key("kept"));
}

#[test]
fn remote_partial_invalid_and_unready_indexes_cannot_remove_missing_threads() {
    let good = json!({"runnerId":"cloud","threadId":"12345678-1234-1234-1234-123456789abc"});
    for value in [json!({}), json!({"threads":false}), json!({"threads":[false]}), json!({"threads":[good.clone(),false]}), json!({"threads":[good.clone(),good.clone()]})] {
        assert!(!decode_remote_threads(&value).can_remove_missing());
    }
    assert!(decode_remote_threads(&json!({"threads":[good]})).can_remove_missing());
    assert!(decode_remote_threads(&json!({"threads":[], "ready":true})).can_remove_missing(), "a confirmed empty remote index removes its last deleted thread");
    assert!(decode_remote_threads(&json!({"threads":[]})).can_remove_missing(), "old runners omit ready once the list has arrived");
    assert!(!decode_remote_threads(&json!({"threads":[], "ready":false})).can_remove_missing());
}

#[test]
fn unreadable_button_actions_never_become_ordinary_reply_pills() {
    use threadbear_core::reply_buttons::ReplyButton;
    for action in ["start", "send", "run"] {
        let mut button = json!({"id":"b", "title":"Go"});
        button[action] = json!("PRIVATE SENTINEL");
        assert!(ReplyButton::decode(&button).is_none(), "bad {action} must drop the button");
    }
    for (action, good, fields) in [
        ("start", json!({"text":"Brief", "cwd":"/work"}), vec!["text", "cwd"]),
        ("send", json!({"threadId":"other", "threadTitle":"Other", "text":"Message"}), vec!["threadId", "text"]),
        ("run", json!({"command":"echo hello", "cwd":"/work"}), vec!["command", "cwd"]),
    ] {
        for field in fields {
            let mut invalid = vec![Some(json!(false)), None];
            if action == "run" { invalid.push(Some(json!(""))); }
            for bad in invalid {
                let mut payload = good.clone();
                if let Some(bad) = bad { payload[field] = bad; } else { payload.as_object_mut().unwrap().remove(field); }
                let mut button = json!({"id":"b", "title":"Go"});
                button[action] = payload;
                assert!(ReplyButton::decode(&button).is_none(), "unreadable {action}.{field}");
            }
        }
        let mut button = json!({"id":"b", "title":"Go", "futureField":42});
        button[action] = good;
        assert!(ReplyButton::decode(&button).is_some(), "readable {action} still works");
    }
    assert!(ReplyButton::decode(&json!({"id":"b", "futureAction":{}})).is_none());
    assert!(ReplyButton::decode(&json!({"id":"b", "start":{"text":"Brief","cwd":"/work"}, "run":{"command":"echo hidden","cwd":"/work"}})).is_none());
}

#[test]
fn remote_reply_buttons_keep_the_relays_redacted_payloads() {
    use threadbear_core::reply_buttons::{ReplyButton, look, Look};
    // Exact shapes from relay.ts buttonsOf: briefs and message destinations
    // remain on the owning runner; pressing routes by thread/button ids.
    let start = json!({"id":"abcd1234","title":"Fix it","createdAt":"2026-11-13T10:00:00Z","start":{"text":"","cwd":"threadbear"}});
    let send = json!({"id":"efgh5678","title":"Tell them","createdAt":"2026-11-13T10:00:00Z","send":{"threadId":"","threadTitle":"Deploy","text":""}});
    assert_eq!(look(&ReplyButton::decode(&start).expect("redacted start remains a pill"),false), Look::Start);
    assert_eq!(look(&ReplyButton::decode(&send).expect("redacted send remains a pill"),false), Look::Send);
    let mut pressed = start;
    pressed["pressed"] = json!({"at":"2026-11-13T10:01:00Z","threadId":"started1"});
    assert_eq!(look(&ReplyButton::decode(&pressed).expect("relay press answers update the pill"),false), Look::Pressed);
    assert!(ReplyButton::decode(&json!({"id":"emptycwd","start":{"text":"","cwd":""}})).is_some(), "relay target is optional too");
}

#[test]
fn provider_commands_are_quoted_as_arguments_and_old_hellos_have_defaults() {
    let old = RunnerLogins::from_hello(&json!({}));
    assert_eq!(old.login_command("claude"), "claude auth login");
    assert_eq!(old.login_command("codex"), "codex login");
    assert_eq!(old.login_command("cursor"), "agent login");
    let malformed = RunnerLogins::from_hello(&json!({"providers":[{"id":"cursor","loginCommand":false},false]}));
    assert_eq!(malformed.login_command("cursor"), "agent login");
    let account = sign_in::SignInAccount { id:"cursor-main".into(), provider:"cursor".into(), title:"Cursor".into(), config_dir:String::new() };
    let script = sign_in::script_with_command(&account, "", "agent login; touch /tmp/unwanted $(whoami)");
    assert!(script.contains("agent 'login;' touch /tmp/unwanted '$(whoami)'"));
    let unknown = RunnerLogins::from_hello(&json!({"providers":[{"id":"new","loginCommand":""},{"id":"cursor","loginCommand":""}]}));
    assert_eq!(unknown.login_command("new"), "");
    assert_eq!(unknown.login_command("cursor"), "");
    assert_eq!(old.login_command("unknown"), "");
    assert!(!sign_in::script_with_command(&account, "/opt/agent", "").contains("/opt/agent"));
}

#[test]
fn patch_trust_disclosures_and_nonmain_login_folders_fail_closed() {
    use threadbear_core::highlights::PatchInfo;
    for field in ["helpers", "grants"] {
        for bad in [json!(["readable",false]),json!({"path":"hidden"})] {
            let mut patch = json!({"id":"p","untrusted":"Read first","file":"patch.ts","hash":"hash"});
            patch[field] = bad;
            assert!(serde_json::from_value::<PatchInfo>(patch).is_err());
        }
    }
    for field in ["source", "file", "hash", "untrusted"] {
        let mut patch = json!({"id":"p","source":"project","untrusted":"Read first","file":"patch.ts","hash":"hash"});
        patch[field] = json!(false);
        assert!(serde_json::from_value::<PatchInfo>(patch).is_err());
    }
    let valid = json!({"id":"p","source":"claude","untrusted":"Read first","file":"patch.ts","hash":"hash"});
    assert!(serde_json::from_value::<PatchInfo>(valid.clone()).unwrap().can_trust());
    for field in ["source", "file", "hash"] {
        let mut patch = valid.clone();
        patch.as_object_mut().unwrap().remove(field);
        assert!(!serde_json::from_value::<PatchInfo>(patch).unwrap().can_trust());
    }
    for (id, provider) in [("claude-1","claude"),("codex-1","codex")] {
        let logins = RunnerLogins::from_hello(&json!({"accounts":[{"id":id,"provider":provider,"configDir":false}]}));
        let a = &logins.accounts[0];
        let account = sign_in::SignInAccount { id:a.id.clone(),provider:a.provider.clone(),title:a.title.clone(),config_dir:a.config_dir.clone() };
        assert!(sign_in::account_for(id,provider,false,Some(account.clone())).is_none());
        let script = sign_in::script(&account,"/opt/cli");
        assert!(!script.contains("/opt/cli"));
    }
    for (id, provider) in [("claude-2","claude"),("codex-2","codex")] {
        for field in ["id", "provider"] {
            let mut row = json!({"id":id,"provider":provider,"configDir":"/secondary"});
            row[field] = json!({});
            let malformed = RunnerLogins::from_hello(&json!({"accounts":[row]}));
            assert!(malformed.accounts.is_empty(), "an unreadable identity or provider drops the account");
            let known = malformed.accounts.iter().find(|a| a.id == id).map(|a| sign_in::SignInAccount { id:a.id.clone(), provider:a.provider.clone(), title:a.title.clone(), config_dir:a.config_dir.clone() });
            assert!(sign_in::account_for(id,provider,false,known).is_none(), "a dropped secondary login cannot sign in to the main login");
        }
        assert!(sign_in::account_for("",provider,false,None).is_none(), "a malformed summary accountId cannot select main either");
        let known = sign_in::SignInAccount { id:id.into(), provider:provider.into(), title:String::new(), config_dir:"/secondary".into() };
        assert_eq!(sign_in::account_for(id,provider,false,Some(known.clone())), Some(known.clone()));
        assert!(sign_in::account_for("different",provider,false,Some(known)).is_none());
        let main = format!("{provider}-main");
        let known = sign_in::SignInAccount { id:main.clone(), provider:provider.into(), title:String::new(), config_dir:"/main".into() };
        assert_eq!(sign_in::account_for(&main,provider,false,Some(known.clone())), Some(known));
    }
    let mismatch = sign_in::SignInAccount { id:"codex-1".into(), provider:"claude".into(), title:String::new(), config_dir:"/codex".into() };
    assert!(sign_in::account_for("codex-1","codex",false,Some(mismatch)).is_none());
}

#[test]
fn missing_main_login_metadata_never_guesses_a_credential_folder() {
    for provider in ["claude", "codex"] {
        let id = format!("{provider}-main");
        assert!(sign_in::account_for(&id,provider,false,None).is_none(), "a dropped account is not permission to guess its configuration");
    }
    for config_dir in [json!(false),json!(null),json!("")] {
        let logins = RunnerLogins::from_hello(&json!({"accounts":[{"id":"codex-main","provider":"codex","configDir":config_dir}]}));
        let a = &logins.accounts[0];
        let account = sign_in::SignInAccount { id:a.id.clone(), provider:a.provider.clone(), title:a.title.clone(), config_dir:a.config_dir.clone() };
        assert!(sign_in::account_for("codex-main","codex",false,Some(account.clone())).is_none());
        assert!(!sign_in::script(&account,"/opt/codex").contains("/opt/codex"), "Codex's main home can be customized too");
    }
}

#[test]
fn malformed_summary_fields_keep_the_thread_and_good_nested_rows() {
    let t: ThreadSummary = serde_json::from_value(json!({
        "id":"good", "title":"Keep this thread", "contextTokens":"bad",
        "sample":[], "projects":[{"id":"p","title":"Project","pinned":"bad"}, 42],
        "asks":[{"action":"Choose", "suggested":"bad", "options":["Yes",42,"No"]}, false],
        "carryOnAtReset":"bad", "futureField":42
    })).expect("one malformed field cannot hide a thread");
    assert_eq!(t.title, "Keep this thread");
    assert_eq!(t.context_tokens, None);
    assert_eq!(t.projects.as_ref().unwrap().len(), 1);
    assert_eq!(t.projects.unwrap()[0].title, "Project");
    let asks = t.asks.unwrap();
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].options, None, "never shift choice positions past a bad label");
    assert!(t.carry_on_at_reset);
}

#[test]
fn remote_summaries_keep_search_words_and_creation_date() {
    let remote = decode_remote_threads(&json!({"threads":[{
        "runnerId":"cloud", "threadId":"12345678-1234-1234-1234-123456789abc",
        "title":"A thread", "keywords":"unusual-search-word", "createdAt":"2026-11-01T10:00:00Z"
    }]}));
    let t = &remote.threads[0];
    assert!(threadbear_core::inbox::matches(t, "unusual-search-word"));
    assert_eq!(t.created_at.as_deref(), Some("2026-11-01T10:00:00Z"));
}

#[test]
fn provider_sign_in_uses_hello_login_command() {
    let logins = RunnerLogins::from_hello(&json!({"providers":[{"id":"cursor", "loginCommand":"agent login"}]}));
    let account = sign_in::SignInAccount { id:"cursor-main".into(), provider:"cursor".into(), title:"Cursor".into(), config_dir:String::new() };
    let script = sign_in::script_with_command(&account, "/opt/agent", logins.login_command("cursor"));
    assert!(script.contains("'/opt/agent' login\n"), "{script}");
    assert!(!script.contains("auth login"));
}

#[test]
fn summary_activity_uses_updated_then_created_and_counts_accept_numeric_strings() {
    let decode = |v| serde_json::from_value::<ThreadSummary>(v).unwrap();
    let summary = decode(json!({"id":"t", "createdAt":"2026-11-01T00:00:00Z", "updatedAt":"2026-11-02T00:00:00Z", "lastSeq":"12", "lastSeenSeq":"10", "asksSeq":"11", "pendingReviews":"2"}));
    assert_eq!(summary.activity_ms(), 1_793_577_600_000);
    let remote = decode_remote_threads(&json!({"threads":[{"runnerId":"r", "threadId":"12345678-1234-1234-1234-123456789abc", "createdAt":"2026-11-01T00:00:00Z", "updatedAt":"2026-11-02T00:00:00Z"}]}));
    assert_eq!(remote.threads[0].activity_ms(), summary.activity_ms());

    assert_eq!((summary.last_seq, summary.last_seen_seq, summary.asks_seq, summary.pending_reviews), (12, 10, Some(11), Some(2)));
    assert!(summary.unread());
    let summary = decode(json!({"id":"t", "lastActivityAt":"bad", "updatedAt":"bad", "createdAt":"2026-11-01T00:00:00Z"}));
    assert_eq!(summary.activity_ms(), 1_793_491_200_000);
    let summary = decode(json!({"id":"t", "lastActivityAt":"2026-11-03T00:00:00Z", "updatedAt":"2026-11-02T00:00:00Z"}));
    assert_eq!(summary.activity_ms(), 1_793_664_000_000);
    for bad in [json!("-1"), json!("1.5"), json!(-1), json!(1.5), json!("9007199254740992"), json!(false), json!({})] {
        let summary = decode(json!({"id":"t", "lastSeq":bad, "pendingReviews":bad}));
        assert_eq!(summary.last_seq, 0);
        assert_eq!(summary.pending_reviews, None);
    }
}

#[test]
fn provider_capabilities_follow_hello_and_fall_back_field_by_field() {
    use threadbear_core::logins::{ChipSettings, ModelChoice};
    let logins = RunnerLogins::from_hello(&json!({"providers":[
        {"id":"new", "name":"New agent", "cliName":"New CLI", "autoMode":"classifier", "effort":"codex", "rewind":true},
        {"id":"claude", "name":42, "autoMode":"asks", "effort":"none", "rewind":false},
        {"id":"codex", "autoMode":42, "effort":"future", "rewind":42}, false
    ]}));
    assert_eq!(logins.provider_title("new"), "New agent");
    assert_eq!(logins.cli_name("new"), "New CLI");
    let clis = threadbear_core::updates::decode_clis_with(&json!([{"provider":"new", "installedVersion":"1.0", "status":"current"}]), &logins);
    assert_eq!(threadbear_core::updates::status_line(&clis[0]), "New CLI 1.0, up to date");
    let command = threadbear_core::slash::SlashCommand { name:"status".into(), source:"cli".into(), scope:Some("system".into()), ..Default::default() };
    assert_eq!(threadbear_core::slash::origin_named(&command, &logins.cli_name("new")), "New CLI's own");

    assert!(logins.has_auto_mode("new") && logins.can_rewind("new"));
    assert_eq!(logins.effort_levels(None, &[], "new").last().map(String::as_str), Some("ultra"));
    assert!(!logins.has_auto_mode("claude"));
    assert!(!logins.can_rewind("claude"));
    assert_eq!(logins.provider_title("claude"), "Claude");
    assert!(logins.effort_levels(None, &[], "claude").is_empty());
    assert!(!logins.has_auto_mode("codex") && logins.can_rewind("codex"));
    assert_eq!(logins.effort_levels(None, &[], "codex").last().map(String::as_str), Some("ultra"));
    let choices = vec![ModelChoice { id:"m".into(), effort_levels:vec!["low".into()], ..Default::default() }];
    assert_eq!(logins.effort_levels(Some("m"), &choices, "new"), ["low"]);
    assert!(logins.effort_levels(Some("m"), &choices, "claude").is_empty(), "provider effort none overrides even a stale model list");
    let settings = ChipSettings { logins:&logins, account:String::new(), provider:"new".into(), pool_id:None, model:None, model_in_use:None, effort:None, effort_in_use:None, permission_mode:"full".into(), cwd:String::new(), is_new:false };
    assert!(settings.permission_modes().contains(&"auto"));
    let old = RunnerLogins::from_hello(&json!({}));
    assert!(old.has_auto_mode("claude") && old.can_rewind("codex"));
    assert!(!old.has_auto_mode("new") && !old.can_rewind("new"));
    assert!(old.effort_levels(None, &[], "new").is_empty());
}
