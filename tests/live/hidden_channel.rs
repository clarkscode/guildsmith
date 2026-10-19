//! Quickstart §6: the hidden-channel guard (research R6).
//!
//! Needs a bot **without** Administrator and a private channel it can't view: set
//! `GUILDSMITH_LIVE_HIDDEN_CHANNEL` to that channel's id.

use crate::{live, run, temp_file};

#[test]
fn a_hidden_channel_id_is_an_error_not_a_create() {
    let Some(g) = live() else { return };
    let Ok(id) = std::env::var("GUILDSMITH_LIVE_HIDDEN_CHANNEL") else {
        eprintln!("skipped: needs GUILDSMITH_LIVE_HIDDEN_CHANNEL (see the module docs)");
        return;
    };
    let file = temp_file("hidden.toml", &format!("[[channels]]\nid = \"{id}\"\nname = \"hidden\"\nkind = \"text\"\n"));
    let p = run(&["plan", "-g", &g, &file], &[], "");
    assert_eq!(p.code, Some(1), "{}{}", p.stdout, p.stderr);
    assert!(p.stderr.contains("missing or hidden from the bot"), "{}", p.stderr);
}
