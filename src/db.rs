use postgres::{Client, Error, NoTls};

pub struct PostgresNode {
    pub addr: String,
    pub user: String,
    pub dbname: String,
}

pub fn connect_to(pg: &PostgresNode) -> Result<Client, Error> {
    let params = format!("postgresql://{}@{}/{}", pg.user, pg.addr, pg.dbname);
    Client::connect(&params, NoTls)
}