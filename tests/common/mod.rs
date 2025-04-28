use nixcp::store::Store;

pub struct Context {
    pub store: Store,
}

impl Context {
    fn new() -> Self {
        let store = Store::connect().expect("connect to nix store");
        Self { store }
    }
}

pub fn context() -> Context {
    Context::new()
}
