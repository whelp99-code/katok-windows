use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn send_help_documents_windows_ui_path_and_dry_run() {
    let mut cmd = Command::cargo_bin("katok").expect("katok binary");
    cmd.args(["send", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("KakaoTalk"))
        .stdout(predicate::str::contains("Windows"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--room"))
        .stdout(predicate::str::contains("--chat"))
        .stdout(predicate::str::contains("logged in"))
        .stdout(predicate::str::contains("not a login tool"))
        .stdout(predicate::str::contains("macOS").or(predicate::str::contains("Mac")));
}

#[test]
fn send_requires_room_or_chat() {
    let mut cmd = Command::cargo_bin("katok").expect("katok binary");
    cmd.args(["send", "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--room").or(predicate::str::contains("--chat")));
}

#[test]
fn send_dry_run_without_kakaotalk_fails_clearly() {
    let mut cmd = Command::cargo_bin("katok").expect("katok binary");
    cmd.args(["send", "--room", "제피란더스", "--dry-run", "--json"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("KakaoTalk.exe")
                .or(predicate::str::contains("Windows desktop"))
                .or(predicate::str::contains("not available")),
        );
}
