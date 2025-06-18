use spacetimedb::{reducer, table, Identity, ReducerContext, Table, Timestamp};

use spacetimedb::{client_visibility_filter, Filter};

/// A client can only see their account
#[client_visibility_filter]
const ACCOUNT_FILTER: Filter = Filter::Sql("SELECT * FROM user WHERE identity = :sender");

/// Only authorized clients can see messages
#[client_visibility_filter]
const MESSAGE_FILTER: Filter = Filter::Sql(
    r#"
    SELECT m.*
    FROM message m
    JOIN user u ON u.dummy_join = m.dummy_join
    WHERE u.authorized = true AND u.identity = :sender
"#,
);

#[table(name = user, public)]
pub struct User {
    #[primary_key]
    identity: Identity,
    name: Option<String>,
    online: bool,
    authorized: bool,
    dummy_join: bool, // workaround join restriction
}

#[table(name = message, public)]
pub struct Message {
    sender: Identity,
    sent: Timestamp,
    text: String,
    dummy_join: bool, // workaround join restriction
}

#[spacetimedb::reducer(init)]
/// Called when the module is initially published
pub fn init(ctx: &ReducerContext) {
    // Create an initial authorized user
    let admin_hex_id = "c2009546b62e8bf62a4b1387664842c54821f56214e6e6897021091f3f5a053f";
    let identity = Identity::from_hex(admin_hex_id).expect("Invalid hex string");
    ctx.db.user().insert(User {
        name: None,
        identity: identity,
        online: true,
        authorized: true,
        dummy_join: true,
    });
}

#[reducer]
/// Clients invoke this reducer to set their user names.
pub fn set_name(ctx: &ReducerContext, name: String) -> Result<(), String> {
    validate_identity(ctx)?;

    if let Some(user) = ctx.db.user().identity().find(ctx.sender) {
        let name = validate_name(name)?;
        ctx.db.user().identity().update(User {
            name: Some(name),
            ..user
        });
        Ok(())
    } else {
        Err("Cannot set name for unknown user".to_string())
    }
}

fn validate_identity(ctx: &ReducerContext) -> Result<(), String> {
    match ctx.db.user().identity().find(ctx.sender) {
        Some(user) if user.authorized => Ok(()),
        Some(_) => Err("Unauthorized user attempted to perform an action".to_string()),
        None => Err("Validation failed: Unknown user".to_string()),
    }
}

/// Takes a name and checks if it's acceptable as a user's name.
fn validate_name(name: String) -> Result<String, String> {
    if name.is_empty() {
        Err("Names must not be empty".to_string())
    } else {
        Ok(name)
    }
}

#[reducer]
/// Clients invoke this reducer to send messages.
pub fn send_message(ctx: &ReducerContext, text: String) -> Result<(), String> {
    validate_identity(ctx)?;

    let text = validate_message(text)?;
    log::info!("{}", text);
    ctx.db.message().insert(Message {
        sender: ctx.sender,
        text,
        sent: ctx.timestamp,
        dummy_join: true,
    });
    Ok(())
}

/// Takes a message's text and checks if it's acceptable to send.
fn validate_message(text: String) -> Result<String, String> {
    if text.is_empty() {
        Err("Messages must not be empty".to_string())
    } else {
        Ok(text)
    }
}

#[reducer(client_connected)]
// Called when a client connects to a SpacetimeDB database server
pub fn client_connected(ctx: &ReducerContext) {
    if let Some(user) = ctx.db.user().identity().find(ctx.sender) {
        // If this is a returning user, i.e. we already have a `User` with this `Identity`,
        // set `online: true`, but leave `name` and `identity` unchanged.
        ctx.db.user().identity().update(User {
            online: true,
            ..user
        });
    } else {
        // If this is a new user, create a `User` row for the `Identity`,
        // which is online, but hasn't set a name.
        ctx.db.user().insert(User {
            name: None,
            identity: ctx.sender,
            online: true,
            authorized: false,
            dummy_join: true,
        });
    }

    if let Some(user) = ctx.db.user().identity().find(ctx.sender) {
        if !user.authorized {
            log::warn!("Unauthorized user connected: {:?}", user.identity.to_hex());
        }
    }
}

#[reducer(client_disconnected)]
// Called when a client disconnects from SpacetimeDB database server
pub fn identity_disconnected(ctx: &ReducerContext) {
    if let Some(user) = ctx.db.user().identity().find(ctx.sender) {
        ctx.db.user().identity().update(User {
            online: false,
            ..user
        });
    } else {
        // This branch should be unreachable,
        // as it doesn't make sense for a client to disconnect without connecting first.
        log::warn!(
            "Disconnect event for unknown user with identity {:?}",
            ctx.sender
        );
    }
}
