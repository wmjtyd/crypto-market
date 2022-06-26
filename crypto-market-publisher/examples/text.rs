fn main() {
    let data = vec![71u8, 69u8, 84u8, 32u8, 47u8, 13u8, 10u8];
    println!("{}", String::from_utf8(data).unwrap());
}