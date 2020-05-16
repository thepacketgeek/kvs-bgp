use std::sync::Arc;

use log::debug;
use tokio::sync::Mutex;
use warp::{self, Filter};

use crate::store::KvStore;

type Store = Arc<Mutex<KvStore>>;

/// API call to get a key (if it exists)
pub async fn get_key(key: String, store: Store) -> Result<impl warp::Reply, warp::Rejection> {
    debug!("GET: {}", key);
    store
        .lock()
        .await
        .get(&key)
        .map(|value| warp::reply::with_status(format!("{}\n", value), warp::http::StatusCode::OK))
        .ok_or_else(|| warp::reject::not_found())
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
) -> Result<impl warp::Reply, warp::Rejection> {
    debug!("INSERT: {} | {}", key, value);
    store
        .lock()
        .await
        .insert(key, value)
        .map_err(|e| warp::reject::custom(e))
        .and_then(|_| Ok(warp::reply()))
}

/// API call to remove a key/value pair by key
///
/// This will trigger a BGP update to peers to:
/// - Withdraw the key/value pair
pub async fn remove_pair(key: String, store: Store) -> Result<impl warp::Reply, warp::Rejection> {
    debug!("REMOVE: {}", key);
    store
        .lock()
        .await
        .remove(&key)
        .map_err(|e| warp::reject::custom(e))
        .and_then(|result| {
            if let Some(value) = result {
                Ok(warp::reply::with_status(
                    format!("{}\n", value),
                    warp::http::StatusCode::OK,
                ))
            } else {
                Err(warp::reject::not_found())
            }
        })
}

/// Defined API routes for Key/Value CRUD
pub fn get_routes(store: Store) -> warp::filters::BoxedFilter<(impl warp::Reply,)> {
    let state = warp::any().map(move || store.clone());
    let status = warp::path!("status").map(|| "Alive!\n".to_owned());

    let get_key = warp::get()
        .and(warp::path!("get" / String))
        .and(warp::path::end())
        .and(state.clone())
        .and_then(get_key);

    let insert_key = warp::put()
        .and(warp::path!("insert" / String / String))
        .and(warp::path::end())
        .and(state.clone())
        .and_then(insert_pair);

    let remove = warp::delete()
        .and(warp::path!("remove" / String))
        .and(warp::path::end())
        .and(state.clone())
        .and_then(remove_pair);

    status.or(get_key).or(insert_key).or(remove).boxed()
}
