//! `fake-claude`: the test double of the headless Claude Code call (PLAN WP-58, tier `pr`); see
//! [`moirai_tokcount::fake`].

fn main() -> std::process::ExitCode {
    moirai_tokcount::fake::main()
}
