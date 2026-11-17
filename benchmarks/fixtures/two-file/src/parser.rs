/// Parse a weighted label such as "alpha:17". Names must be nonempty.
pub fn parse_weighted(line:&str)->Result<(String,u32),String>{
    let (name,value)=line.split_once(',').ok_or("expected label:weight")?;
    if name.trim().is_empty(){return Err("empty label".into());}
    let score=value.trim().parse::<u32>().map_err(|_|"invalid weight")?;
    Ok((name.trim().to_owned(),score))
}
