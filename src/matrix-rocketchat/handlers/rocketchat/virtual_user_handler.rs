use std::convert::TryFrom;

use diesel::sqlite::SqliteConnection;
use ruma_identifiers::{RoomId, UserId};
use slog::Logger;

use api::MatrixApi;
use config::Config;
use db::{NewUser, NewUserOnRocketchatServer, User, UserOnRocketchatServer};
use errors::*;
use i18n::*;

/// Provides helper methods to manage virtual users.
pub struct VirtualUserHandler<'a> {
    /// Application service configuration
    pub config: &'a Config,
    /// SQL database connection
    pub connection: &'a SqliteConnection,
    /// Logger context
    pub logger: &'a Logger,
    /// Matrix REST API
    pub matrix_api: &'a Box<MatrixApi>,
}

impl<'a> VirtualUserHandler<'a> {
    /// Add a virtual user to a Matrix room
    pub fn add_to_room(&self, matrix_user_id: UserId, matrix_room_id: RoomId) -> Result<()> {
        self.matrix_api.invite(matrix_room_id.clone(), matrix_user_id.clone())?;
        self.matrix_api.join(matrix_room_id.clone(), matrix_user_id.clone())?;
        Ok(())
    }

    /// Register a virtual user on the Matrix server and assign it to a Rocket.Chat server.
    pub fn find_or_register(&self,
                            rocketchat_server_id: i32,
                            rocketchat_user_id: String,
                            rocketchat_user_name: String)
                            -> Result<UserOnRocketchatServer> {
        let user_id_local_part = format!("{}_{}_{}", self.config.sender_localpart, &rocketchat_user_id, rocketchat_server_id);
        let user_id = format!("@{}:{}", user_id_local_part, self.config.hs_domain);
        let matrix_user_id = UserId::try_from(&user_id).chain_err(|| ErrorKind::InvalidUserId(user_id))?;

        if let Some(user_on_rocketchat_server) =
            UserOnRocketchatServer::find_by_rocketchat_user_id(self.connection,
                                                               rocketchat_server_id.clone(),
                                                               rocketchat_user_id.clone(),
                                                               true)? {
            return Ok(user_on_rocketchat_server);
        }

        let new_user = NewUser {
            language: DEFAULT_LANGUAGE,
            matrix_user_id: matrix_user_id.clone(),
        };
        User::insert(self.connection, &new_user)?;

        let new_user_on_rocketchat_server = NewUserOnRocketchatServer {
            is_virtual_user: true,
            matrix_user_id: matrix_user_id,
            rocketchat_auth_token: None,
            rocketchat_server_id: rocketchat_server_id,
            rocketchat_user_id: Some(rocketchat_user_id.clone()),
            rocketchat_username: Some(rocketchat_user_name.clone()),
        };
        let user_on_rocketchat_server = UserOnRocketchatServer::upsert(self.connection, &new_user_on_rocketchat_server)?;

        self.matrix_api.register(user_id_local_part.clone())?;
        if let Err(err) = self.matrix_api.set_display_name(user_on_rocketchat_server.matrix_user_id.clone(),
                                                           rocketchat_user_name.clone()) {
            info!(self.logger,
                  format!("Setting display name `{}`, for user `{}` failed with {}",
                          &user_on_rocketchat_server.matrix_user_id,
                          &rocketchat_user_name,
                          err));
        }

        Ok(user_on_rocketchat_server)
    }
}