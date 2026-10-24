//! `delphi-core`: the daemon as its own executable, for debugging and the daemon tests. The `delphi` program
//! runs the same code as `delphi __core`, and is the only executable an install needs.

fn main() {
    delphi_engine::core_main::run(std::env::args().skip(1).collect());
}
