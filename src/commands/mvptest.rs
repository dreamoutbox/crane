use crate::server_interactor::server_interactor_trait::ServerInteractor;

/// Connects using the interactor and executes `whoami`.
pub fn run(interactor: &dyn ServerInteractor) -> anyhow::Result<()> {
    println!("Executing 'whoami' via server interactor...");
    let user = interactor.whoami()?;

    let echo1 = interactor.cmd("echo 1 > /home/admin/test.txt")?;
    let echo1_check = interactor.read_file("/home/admin/test.txt")?;

    println!("{}", user);

    dbg!("{}", echo1);
    dbg!("{}", echo1_check);

    Ok(())
}
