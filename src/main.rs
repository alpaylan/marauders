use std::borrow::{Borrow, BorrowMut};

mod languages;
mod variation;


fn main() -> Result<(), Box<dyn std::error::Error>>{
    
    let code = &mut variation::Code::from_file("/Users/akeles/Programming/projects/mutant_ext/mutant-rs/test/BST.v")?;

    let variations = code.get_variations();

    for v in variations {
        println!("{:?}", v);
    }

    if let variation::CodePart::Variation(ref mut v) = code.parts[1].borrow_mut() {
        v.active = 1;
    } else {
        println!("Not a variant");
    }


    let variations = code.get_variations();

    for v in variations {
        println!("{:?}", v);
    }

    print!("{}", code);
    Ok(())
}
