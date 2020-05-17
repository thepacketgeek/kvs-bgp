use std::sync::Arc;

use log::debug;
use tokio::sync::{mpsc, RwLock};
use warp::{self, Filter};

use crate::store::{KvStore, Update};

type Store = Arc<RwLock<KvStore>>;
type UpdateChannel = mpsc::UnboundedSender<Update>;

/// API call to get a key (if it exists)
pub async fn get_key(key: String, store: Store) -> Result<impl warp::Reply, warp::Rejection> {
    debug!("GET: {}", key);
    store
        .read()
        .await
        .get(&key)
        .map(|value| warp::reply::with_status(format!("{}\n", value), warp::http::StatusCode::OK))
        .ok_or_else(warp::reject::not_found)
}

/// API call to insert/update a key/value pair
///
/// This will trigger a BGP update to peers to:
/// - Announce the new/updated key
/// - Withdraw the existing value (if this is a value update)
pub async fn insert_pair(
    key: String,
    value: String,
    store: Store,
    channel: UpdateChannel,
) -> Result<impl warp::Reply, warp::Rejection> {
    debug!("INSERT: {} | {}", key, value);
    store
        .write()
        .await
        .insert(key, value)
        .map_err(warp::reject::custom)
        .and_then(|update| {
            channel.send(update).unwrap();
            Ok(warp::reply())
        })
}

/// API call to remove a key/value pair by key
///
/// This will trigger a BGP update to peers to:
/// - Withdraw the key/value pair
pub async fn remove_pair(
    key: String,
    store: Store,
    channel: UpdateChannel,
) -> Result<impl warp::Reply, warp::Rejection> {
    debug!("REMOVE: {}", key);
    store
        .write()
        .await
        .remove(&key)
        .map_err(warp::reject::custom)
        .and_then(|result| {
            if let Some(update) = result {
                channel.send(update).unwrap();
                Ok(warp::reply::with_status("", warp::http::StatusCode::OK))
            } else {
                Err(warp::reject::not_found())
            }
        })
}

/// Defined API routes for Key/Value CRUD
pub fn get_routes(
    store: Store,
    channel: UpdateChannel,
) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
    let store = warp::any().map(move || store.clone());
    let channel = warp::any().map(move || channel.clone());

    let status = warp::path!("status").map(|| "Alive!\n".to_owned());

    let get_key = warp::get()
        .and(warp::path!("get" / String))
        .and(warp::path::end())
        .and(store.clone())
        .and_then(get_key);

    let insert_key = warp::put()
        .and(warp::path!("insert" / String / String))
        .and(warp::path::end())
        .and(store.clone())
        .and(channel.clone())
        .and_then(insert_pair);

    let remove = warp::delete()
        .and(warp::path!("remove" / String))
        .and(warp::path::end())
        .and(store.clone())
        .and(channel.clone())
        .and_then(remove_pair);

    status.or(get_key).or(insert_key).or(remove).boxed()
}
