use mlua::FromLua;
use pax_derive::UserData;

#[derive(UserData, Debug, PartialEq)]
enum Letters {
    A,
    B,
    C,
}

fn main() {
    let lua = mlua::Lua::new();
    lua.globals().set("Letters", Letters::A).unwrap();
    for tt in [("A", 0), ("B", 1), ("C", 2)] {
        let res: usize = lua.load(format!("return Letters.{}", tt.0)).eval().unwrap();
        assert_eq!(res, tt.1)
    }
    for tt in [("A", Letters::A), ("B", Letters::B), ("C", Letters::C)] {
        let eval: Letters = lua.load(format!("return Letters.{}", tt.0)).eval().unwrap();
        assert_eq!(eval, tt.1);
        let s = lua.create_string(tt.0).unwrap();
        let l0 = Letters::from_lua(mlua::Value::String(s.clone()), &lua).unwrap();
        let l1 = Letters::try_from(s.clone()).unwrap();
        assert_eq!(l0, tt.1);
        assert_eq!(l1, tt.1);
    }
    assert!(lua.load("return Letters.is_enum()").eval::<bool>().unwrap());
    assert_eq!(
        lua.load("return Letters.variants()")
            .eval::<Vec<String>>()
            .unwrap(),
        vec!["A", "B", "C"]
    );
}
