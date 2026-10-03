pub trait Adapter {
    fn search(&self);
}

pub trait Other {
    fn search(&self);
}

pub struct Recall;

impl Recall {
    pub fn search(&self) {
        <Self as Adapter>::search(self);
    }
}

impl Adapter for Recall {
    fn search(&self) {}
}

impl Other for Recall {
    fn search(&self) {}
}

pub struct Generic<T>(pub T);

impl<T> Adapter for Generic<T> {
    fn search(&self) {}
}

pub mod nested {
    pub fn search() {
        super::Recall.search();
    }
}

pub fn search() {
    nested::search();
}
