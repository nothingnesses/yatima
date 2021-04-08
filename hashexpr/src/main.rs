// use atom::Atom;
use hashexpr::*;
use std::io::{self, Read};
// use span::Span;

fn main() -> io::Result<()> {
  let mut source = String::new();
  let stdin = io::stdin();
  let mut handle = stdin.lock();
  handle.read_to_string(&mut source)?;
  let result = parse(source.as_str());
  match result {
    Ok(he) => {
      println!("{}", he.1);  
    }
    Err(res) => {
      eprintln!("{}", res)
    }
  }

  Ok(())
}
