fn main() {
    let value = parse();
    save(value);
}

fn parse() -> u32 {
    helper()
}

fn save(_value: u32) {}

fn helper() -> u32 {
    42
}
