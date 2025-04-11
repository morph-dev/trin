use discv5::Enr;
use portalnet::discovery::ENR_PORTAL_CLIENT_KEY;

pub fn get_client_info(peer: &Enr) -> String {
    match peer
        .get_decodable::<String>(ENR_PORTAL_CLIENT_KEY)
        .and_then(|client| client.ok())
    {
        Some(client) => client,
        None => "?".to_string(),
    }
}
