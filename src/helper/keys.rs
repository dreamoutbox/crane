use crate::config;

pub fn find_private_key_for_user(
    username: &str,
    config: &config::Config,
) -> anyhow::Result<String> {
    // Search in node configurations
    for node in &config.nodes {
        if node.user == username {
            let pk = &node.private_key;
            return Ok(pk.clone());
        }
    }

    // Search in user configurations
    // if let Some(ref users) = config.users {
    //     for user in users {
    //         if user.name == username {
    //             if let Some(ref pk) = user.private_key {
    //                 return Ok(pk.clone());
    //             }
    //         }
    //     }
    // }

    anyhow::bail!("No private key found for user '{}'", username)
}
