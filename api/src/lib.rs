use consensus::consensus_controller::ConsensusControllerInterface;
use crypto::hash::Hash;
use std::collections::HashSet;
use warp::{reject, Filter};

#[derive(Debug)]
struct InternalError {
    message: String,
}

impl reject::Reject for InternalError {}

pub async fn serve<ConsensusControllerInterfaceT: ConsensusControllerInterface + 'static>(
    consensus_controller_interface: ConsensusControllerInterfaceT,
) {
    let block = warp::get()
        .and(warp::path("api"))
        .and(warp::path("v1"))
        .and(warp::path("block"))
        .and(warp::path::param::<Hash>()) //block hash
        //.and(warp::path::end())
        .and_then(move |hash| get_block(hash, consensus_controller_interface.clone()));

    /*let blockinterval = warp::get()
    .and(warp::path("api"))
    .and(warp::path("v1"))
    .and(warp::path("blockinterval"))
    .and(warp::path::param::<String>()) //block hash
    //.and(warp::path::end())
    .and_then(|hash| block(hash, &*interface)); */

    //  let routes = block.or(blockinterval);

    warp::serve(block).run(([127, 0, 0, 1], 3030)).await;
}

async fn get_block<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    hash: Hash,
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    match interface.get_active_block(hash).await {
        Err(err) => Err(warp::reject::custom(InternalError {
            message: err.to_string(),
        })),
        Ok(None) => Err(warp::reject::not_found()),
        Ok(Some(block)) => Ok(warp::reply::json(&block)),
    }
}

async fn current_parents<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let parents = interface
        .get_block_graph_status()
        .await
        .map_err(|_| warp::reject::reject())?
        .best_parents;
    Ok(warp::reply::json(&parents))
}

async fn last_final<ConsensusControllerInterfaceT: ConsensusControllerInterface>(
    interface: ConsensusControllerInterfaceT,
) -> Result<impl warp::Reply, warp::Rejection> {
    let finals = interface
        .get_block_graph_status()
        .await
        .map_err(|_| warp::reject::reject())?;
    let finals = finals
        .latest_final_blocks_periods
        .iter()
        .enumerate()
        .map(|(i, (hash, period))| (hash, *period, i as u8))
        .collect::<HashSet<(&Hash, u64, u8)>>();
    Ok(warp::reply::json(&finals))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_server() {
        //serve().await;
    }
}
