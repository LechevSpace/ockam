use crate::node::NodeOpts;
use crate::util::{api, connect_to, exitcode, extract_address_value};
use crate::CommandGlobalOpts;
use clap::Args;
use cli_table::{print_stdout, Cell, Style, Table};
use ockam::{Context, Route};
use ockam_api::nodes::{
    models::transport::{TransportList, TransportStatus},
    NODEMANAGER_ADDR,
};

#[derive(Args, Clone, Debug)]
pub struct ListCommand {
    #[command(flatten)]
    node_opts: NodeOpts,
}

impl ListCommand {
    pub fn run(self, options: CommandGlobalOpts) {
        let cfg = &options.config;
        let node =
            extract_address_value(&self.node_opts.api_node).unwrap_or_else(|_| "".to_string());
        let port = cfg.get_node_port(&node).unwrap();

        connect_to(port, (), list_connections);
    }
}

pub async fn list_connections(ctx: Context, _: (), mut base_route: Route) -> anyhow::Result<()> {
    let resp: Vec<u8> = match ctx
        .send_and_receive(
            base_route.modify().append(NODEMANAGER_ADDR),
            api::list_tcp_connections()?,
        )
        .await
    {
        Ok(sr_msg) => sr_msg,
        Err(e) => {
            eprintln!("Wasn't able to send or receive `Message`: {}", e);
            std::process::exit(exitcode::IOERR);
        }
    };

    let TransportList { list, .. } = api::parse_tcp_list(&resp)?;

    let table = list
        .iter()
        .fold(
            vec![],
            |mut acc,
             TransportStatus {
                 tt,
                 tm,
                 payload,
                 tid,
                 ..
             }| {
                let row = vec![tid.cell(), tt.cell(), tm.cell(), payload.cell()];
                acc.push(row);
                acc
            },
        )
        .table()
        .title(vec![
            "Transport ID".cell().bold(true),
            "Transport Type".cell().bold(true),
            "Mode".cell().bold(true),
            "Address bind".cell().bold(true),
        ]);

    if let Err(e) = print_stdout(table) {
        eprintln!("failed to print node status: {}", e);
        std::process::exit(exitcode::IOERR);
    }

    Ok(())
}
