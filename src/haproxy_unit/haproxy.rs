use crate::{config, server_interactor::get_server_interactor};

pub async fn reload_haproxy_on_each_nodes_wrapper(
    app_nodes: &Vec<config::NodeConfig>,
) -> Result<(), anyhow::Error> {
    let mut handles = vec![];

    for app_node in app_nodes {
        let app_node = app_node.clone();
        let handle = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            println!("\tReloading HAProxy on app node {}...", app_node.name);

            let interactor = get_server_interactor(&app_node.name)?;

            interactor.reload_haproxy()?;

            Ok(())
        });
        handles.push(handle);
    }

    let mut results = vec![];
    for handle in handles {
        results.push(handle.await);
    }
    for res in results {
        res??;
    }

    Ok(())
}
