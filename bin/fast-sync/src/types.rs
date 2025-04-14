use discv5::Enr;

#[derive(Debug)]
pub enum FindContentResult<TContetnValue> {
    Content(TContetnValue),
    Peers(Vec<Enr>),
}
