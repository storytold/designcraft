//! #776: callers from outside are checked with `check_params`; `Session::execute` itself takes
//! what it is given, so a journal written by an older version still replays.

use designcraft_engine::{Session, check_params};
use serde_json::json;

#[test]
fn execute_replays_what_the_check_refuses() {
    let mut s = Session::new();
    s.execute("file.new", &json!({})).unwrap();
    let params = json!({"bogus": 1});
    assert!(check_params("style.list", &params).is_err());
    assert!(s.execute("style.list", &params).is_ok());
}
