#![allow(missing_docs)]

use std::error::Error as StdError;
use std::fmt::{Display, Formatter};
use std::fmt::Error as FmtError;
use std::result::Result as StdResult;

use diesel::TransactionError;
use iron::{IronError, Response};
use iron::modifier::Modifier;
use iron::status::Status;
use serde_json;

use i18n::*;

macro_rules! simple_error {
    ($e:expr) => {
        Error{
            error_chain: $e.into(),
            user_message: None,
        }
    };
}

macro_rules! user_error {
    ($e:expr, $u:expr) => {
        Error{
            error_chain: $e.into(),
            user_message: Some($u),
        }
    }
}

macro_rules! bail_error {
    ($e:expr) => {
        return Err(simple_error!($e));
    };
    ($e:expr, $u:expr) => {
        return Err(user_error!($e, $u));
    };
}

/// `ErrorResponse` defines the format that is used to send an error response as JSON.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
    causes: Vec<String>,
}

/// Response from the Matrix homeserver when an error occurred
#[derive(Deserialize, Serialize)]
pub struct MatrixErrorResponse {
    /// Error code returned by the Matrix API
    pub errcode: String,
    /// Error message returned by the Matrix API
    pub error: String,
}

/// Response from the Rocket.Chat server when an error occurred
#[derive(Deserialize, Serialize)]
pub struct RocketchatErrorResponse {
    /// Status returned by the Rocket.Chat API
    pub status: Option<String>,
    /// Error message returned by the Rocket.Chat API
    pub message: Option<String>,
    /// The error that occured
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct Error {
    /// The chained errors
    pub error_chain: ErrorChain,
    /// An optional message that is shown to the user
    pub user_message: Option<I18n>,
}

pub type Result<T> = StdResult<T, Error>;

error_chain!{
    types {
        ErrorChain, ErrorKind, ResultExt;
    }

    errors {
        InvalidAccessToken(token: String) {
            description("The provided access token is not valid")
            display("Could not process request, the access token `{}` is not valid", token)
        }

        MissingAccessToken {
            description("The access token is missing")
            display("Could not process request, no access token was provided")
        }

        InvalidRocketchatToken(token: String) {
            description("The provided access token is not valid")
            display("Could not process request, the access token `{}` did not match any bridged Rocket.Chat server", token)
        }

        MissingRocketchatToken {
            description("The Rocket.Chat token is missing")
            display("Could not process request, no Rocket.Chat token provided")
        }

        InvalidJSON(msg: String) {
            description("The provided data is not valid.")
            display("Could not process request, the submitted data is not valid: {}", msg)
        }

        InvalidYAML(msg: String) {
            description("The provided YAML is not valid.")
            display("The submitted data is not valid YAML: {}", msg)
        }

        InvalidUserId(user_id: String) {
            description("The provided user ID is not valid")
            display("The provided user ID {} is not valid", user_id)
        }

        EventIdGenerationFailed{
            description("Generating a new event ID failed")
            display("Could not generate a new event ID")
        }

        UnsupportedHttpMethod(method: String) {
            description("The REST API was called with an unsupported method")
            display("Unsupported HTTP method {}", method)
        }

        AuthenticationFailed(error_msg: String) {
            description("Authentication failed")
            display("Authentication failed: {}", error_msg)
        }

        ApiCallFailed(url: String) {
            description("Call to REST API failed")
            display("Could not call REST API endpoint {}", url)
        }

        MatrixError(error_msg: String) {
            description("Errors returned by the Matrix homeserver")
            display("Matrix error: {}", error_msg)
        }

        UnsupportedMatrixApiVersion(versions: String) {
            description("The homeserver's API version is not compatible with the application service")
            display("No supported API version found for the Matrix homeserver, found versions: {}", versions)
        }

        RocketchatError(error_msg: String) {
            description("Errors returned by the Rocket.Chat API")
            display("Rocket.Chat error: {}", error_msg)
        }

        NoRocketchatServer(url: String){
            description("The server is not a Rocket.Chat server")
            display("No Rocket.Chat server found when querying {} (version information is missing from the response)", url)
        }

        RocketchatServerUnreachable(url: String) {
            description("The Rocket.Chat is not reachable")
            display("Could not reach Rocket.Chat server {}", url)
        }

        UnsupportedRocketchatApiVersion(min_version: String, versions: String) {
            description("The Rocket.Chat server's version is not compatible with the application service")
            display("No supported API version (>= {}) found for the Rocket.Chat server, found version: {}", min_version, versions)
        }

        ReadFileError(path: String) {
            description("Error when reading a file")
            display("Reading file from {} failed", path)
        }

        RoomNotConnected(matrix_room_id: String) {
            description("The room is not connected, but has to be for the command the user submitted")
            display("Room {} is not connected to a Rocket.Chat server, cannot execute command", matrix_room_id)
        }

        RoomAlreadyConnected(matrix_room_id: String) {
            description("The Room is already connected to a Rocket.Chat server")
            display("Room {} is already connected", matrix_room_id)
        }

        AdminRoomForRocketchatServerNotFound(rocketchat_url: String) {
            description("The user does not have an admin room that is connected to the given Rocket.Chat server")
            display("No admin room found that is connected to the Rocket.Chat server {}", rocketchat_url)
        }

        RocketchatTokenMissing{
            description("A token is needed to connect new Rocket.Chat servers")
            display("Attempt to connect a Rocket.Chat server without a token")
        }

        RocketchatServerAlreadyConnected(rocketchat_url: String) {
            description("The Rocket.Chat server is already connected to the application service")
            display("Attempt to connect {}, but the Rocket.Chat server is already connected", rocketchat_url)
        }

        RocketchatTokenAlreadyInUse(token: String) {
            description("The token is already used by another server")
            display("The token {} is already in use by another server", token)
        }

        RocketchatChannelNotFound(channel_name: String) {
            description("No channel with the given name found on the Rocket.Chat server")
            display("The channel {} does not exist on the Rocket.Chat server", channel_name)
        }

        RocketchatChannelAlreadyBridged(channel_name: String) {
            description("The channel with the given name is already bridged")
            display("The channel {} is already bridged", channel_name)
        }

        RocketchatJoinFirst(channel_name: String) {
            description("The user has to join the channel on Rocket.Chat before it can be bridged")
            display("Bridging the channel {} failed, because the user hasn't joined it on Rocket.Chat", channel_name)
        }

        UnbridgeOfNotBridgedRoom(display_name: String) {
            description("Room with the given display name could not be found")
            display("No room with display_name {} found", display_name)
        }

        RoomNotEmpty(display_name: String, users: String) {
            description("Non virtual users are in the room")
            display("The room {} has matrix users ({}) in it, cannot unbridge", display_name, users)
        }

        ReadConfigError {
            description("Error when reading the config content to a string")
            display("Could not read config content to string")
        }

        ServerStartupError {
            description("Error when starting the application service")
            display("Could not start application service")
        }

        DatabaseSetupError {
            description("Error when setting up the database")
            display("Could not setup database")
        }

        MigrationError {
            description("Error when running migrations")
            display("Could not run migrations")
        }

        DBConnectionError {
            description("Error when establishing a connection to the database")
            display("Could not establish database connection")
        }

        LoggerExtractionError {
            description("Error when getting the logger from the request")
            display("Could not get logger from iron")
        }

        ConnectionPoolExtractionError {
            description("Error when getting the connection pool from the request")
            display("Could not get connection pool from iron request")
        }

        ConnectionPoolCreationError {
            description("Error when creating the connection pool")
            display("Could not create connection pool")
        }

        GetConnectionError {
            description("Error when getting a connection from the connection pool")
            display("Could not get connection from connection pool")
        }

        DBTransactionError {
            description("Error when running a transaction")
            display("An error occurred when running the transaction")
        }

        DBInsertError {
            description("Error when inserting a record")
            display("Inserting record into the database failed")
        }

        DBUpdateError {
            description("Error when editing a record")
            display("Editing record in the database failed")
        }

        DBSelectError {
            description("Error when selecting a record")
            display("Select record from the database failed")
        }

        DBDeleteError {
            description("Error when deleting a record")
            display("Deleting record from the database failed")
        }

        InternalServerError {
            description("An internal error")
            display("An internal error occurred")
        }
    }
}

impl Error {
    pub fn status_code(&self) -> Status {
        match *self.error_chain {
            ErrorKind::InvalidAccessToken(_) |
            ErrorKind::InvalidRocketchatToken(_) => Status::Forbidden,
            ErrorKind::MissingAccessToken |
            ErrorKind::MissingRocketchatToken => Status::Unauthorized,
            ErrorKind::InvalidJSON(_) => Status::UnprocessableEntity,
            ErrorKind::AdminRoomForRocketchatServerNotFound(_) => Status::NotFound,
            ErrorKind::AuthenticationFailed(_) => Status::Unauthorized,
            _ => Status::InternalServerError,
        }
    }
}

impl StdError for Error {
    fn description(&self) -> &str {
        self.error_chain.description()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> StdResult<(), FmtError> {
        write!(f, "{}", self.error_chain)
    }
}

impl From<ErrorChain> for Error {
    fn from(error: ErrorChain) -> Error {
        simple_error!(error)
    }
}

impl From<ErrorKind> for Error {
    fn from(error: ErrorKind) -> Error {
        simple_error!(error)
    }
}

impl From<TransactionError<Error>> for Error {
    fn from(error: TransactionError<Error>) -> Error {
        match error {
            TransactionError::UserReturnedError(err) => {
                Error {
                    user_message: err.user_message,
                    error_chain: err.error_chain,
                }
            }
            TransactionError::CouldntCreateTransaction(_) => simple_error!(ErrorKind::DBTransactionError),
        }
    }
}

impl From<Error> for IronError {
    fn from(error: Error) -> IronError {
        let response = Response::with(&error);
        IronError {
            error: Box::new(error),
            response: response,
        }
    }
}

impl<'a> Modifier<Response> for &'a Error {
    fn modify(self, response: &mut Response) {
        let error_message = match self.user_message {
            Some(ref user_message) => user_message.l(DEFAULT_LANGUAGE),
            None => format!("{}", self),
        };

        let causes = self.error_chain.iter().skip(1).map(|e| format!("{}", e)).collect();
        let resp = ErrorResponse {
            error: format!("{}", &error_message),
            causes: causes,
        };

        let err_msg = serde_json::to_string(&resp).expect("ErrorResponse is always serializable");
        response.status = Some(self.status_code());
        response.body = Some(Box::new(err_msg));
    }
}
