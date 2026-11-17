/// Return the highest-scoring label; break equal-score ties alphabetically.
pub fn highest(entries:&[(String,u32)])->Option<String>{
    entries.iter().min_by_key(|(name,score)|(*score,name.clone()))
        .map(|(name,_)|name.clone())
}
