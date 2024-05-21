use pax_derive::UserData;

#[derive(UserData)]
enum Letters {
    A,
    B,
    C(i32),
}

fn main() {}
