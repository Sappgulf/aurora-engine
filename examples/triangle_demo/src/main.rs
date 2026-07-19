//! Aurora Engine smoke test: rotating aurora triangle (native + WASM).

use aurora_engine::{run, TriangleDemo};

fn main() {
    run(TriangleDemo::default());
}
