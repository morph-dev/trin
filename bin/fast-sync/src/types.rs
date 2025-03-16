use discv5::Enr;

#[derive(Debug)]
pub enum FindContentResult<T> {
    Content(T),
    Peers(Vec<Enr>),
}
