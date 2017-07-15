#![feature(try_from)]

extern crate iron;
extern crate matrix_rocketchat;
extern crate matrix_rocketchat_test;
extern crate reqwest;
extern crate router;
extern crate ruma_client_api;
extern crate ruma_identifiers;
extern crate serde_json;

use std::collections::HashMap;
use std::convert::TryFrom;

use iron::status;
use matrix_rocketchat::api::rocketchat::Message;
use matrix_rocketchat::api::rocketchat::v1::DIRECT_MESSAGES_LIST_PATH;
use matrix_rocketchat::db::Room;
use matrix_rocketchat_test::{MessageForwarder, RS_TOKEN, Test, default_timeout, handlers, helpers};
use router::Router;
use ruma_client_api::Endpoint;
use ruma_client_api::r0::account::register::Endpoint as RegisterEndpoint;
use ruma_client_api::r0::membership::forget_room::Endpoint as ForgetRoomEndpoint;
use ruma_client_api::r0::membership::invite_user::Endpoint as InviteEndpoint;
use ruma_client_api::r0::membership::leave_room::Endpoint as LeaveRoomEndpoint;
use ruma_client_api::r0::room::create_room::Endpoint as CreateRoomEndpoint;
use ruma_client_api::r0::send::send_message_event::Endpoint as SendMessageEventEndpoint;
use ruma_identifiers::{RoomId, UserId};
use serde_json::to_string;


#[test]
fn successfully_forwards_a_direct_message() {
    let test = Test::new();
    let (create_room_forwarder, create_room_receiver) = handlers::MatrixCreateRoom::with_forwarder(test.config.as_url.clone());
    let (register_forwarder, register_receiver) = handlers::MatrixRegister::with_forwarder();
    let (invite_forwarder, invite_receiver) = MessageForwarder::new();
    let (message_forwarder, receiver) = MessageForwarder::new();
    let mut matrix_router = test.default_matrix_routes();
    matrix_router.put(SendMessageEventEndpoint::router_path(), message_forwarder, "send_message_event");
    matrix_router.post(CreateRoomEndpoint::router_path(), create_room_forwarder, "create_room");
    matrix_router.post(InviteEndpoint::router_path(), invite_forwarder, "invite_user");
    matrix_router.post(RegisterEndpoint::router_path(), register_forwarder, "register");
    let mut rocketchat_router = Router::new();
    let mut direct_messages = HashMap::new();
    direct_messages.insert("spec_user_id_other_user_id", vec!["spec_user", "other_user"]);
    let direct_messages_list_handler = handlers::RocketchatDirectMessagesList {
        direct_messages: direct_messages,
        status: status::Ok,
    };
    rocketchat_router.get(DIRECT_MESSAGES_LIST_PATH, direct_messages_list_handler, "direct_messages_list");

    let test = test.with_matrix_routes(matrix_router)
        .with_rocketchat_mock()
        .with_custom_rocketchat_routes(rocketchat_router)
        .with_connected_admin_room()
        .with_logged_in_user()
        .run();

    let first_direct_message = Message {
        message_id: "spec_id_1".to_string(),
        token: Some(RS_TOKEN.to_string()),
        channel_id: "spec_user_id_other_user_id".to_string(),
        channel_name: None,
        user_id: "other_user_id".to_string(),
        user_name: "other_user".to_string(),
        text: "Hey there".to_string(),
    };
    let first_direct_message_payload = to_string(&first_direct_message).unwrap();

    helpers::simulate_message_from_rocketchat(&test.config.as_url, &first_direct_message_payload);

    helpers::join(
        &test.config.as_url,
        RoomId::try_from("!1234_id:localhost").unwrap(),
        UserId::try_from("@spec_user:localhost").unwrap(),
    );

    let create_room_message = create_room_receiver.recv_timeout(default_timeout()).unwrap();
    assert!(create_room_message.contains("\"room_alias_name\":\"rocketchat_rc_id_spec_user_id_other_user_id\""));

    // discard bot registration
    register_receiver.recv_timeout(default_timeout()).unwrap();

    let register_message = register_receiver.recv_timeout(default_timeout()).unwrap();
    assert!(register_message.contains("\"username\":\"rocketchat_other_user_id_rc_id\""));

    let spec_user_invite = invite_receiver.recv_timeout(default_timeout()).unwrap();
    assert!(spec_user_invite.contains("\"user_id\":\"@spec_user:localhost\""));
    // the users rocket.chat virtual user is not invited into direct message rooms
    assert!(invite_receiver.recv_timeout(default_timeout()).is_err());

    // discard welcome message
    receiver.recv_timeout(default_timeout()).unwrap();
    // discard connect message
    receiver.recv_timeout(default_timeout()).unwrap();
    // discard login message
    receiver.recv_timeout(default_timeout()).unwrap();

    let first_message_received_by_matrix = receiver.recv_timeout(default_timeout()).unwrap();
    assert!(first_message_received_by_matrix.contains("Hey there"));

    let connection = test.connection_pool.get().unwrap();
    let room = Room::find_by_rocketchat_room_id(&connection, "rc_id".to_string(), "spec_user_id_other_user_id".to_string())
        .unwrap()
        .unwrap();

    let users = room.users(&connection).unwrap();
    assert_eq!(users.len(), 2);
    assert!(users.iter().any(|u| u.matrix_user_id == UserId::try_from("@rocketchat_other_user_id_rc_id:localhost").unwrap()));
    assert!(users.iter().any(|u| u.matrix_user_id == UserId::try_from("@spec_user:localhost").unwrap()));

    let second_direct_message = Message {
        message_id: "spec_id_2".to_string(),
        token: Some(RS_TOKEN.to_string()),
        channel_id: "spec_user_id_other_user_id".to_string(),
        channel_name: None,
        user_id: "other_user_id".to_string(),
        user_name: "other_user".to_string(),
        text: "Yay".to_string(),
    };
    let second_direct_message_payload = to_string(&second_direct_message).unwrap();

    helpers::simulate_message_from_rocketchat(&test.config.as_url, &second_direct_message_payload);

    let second_message_received_by_matrix = receiver.recv_timeout(default_timeout()).unwrap();
    assert!(second_message_received_by_matrix.contains("Yay"));
}

#[test]
fn the_bot_user_stays_in_the_direct_message_room_if_the_user_leaves() {
    let test = Test::new();

    let mut rocketchat_router = Router::new();
    let mut direct_messages = HashMap::new();
    direct_messages.insert("spec_user_id_other_user_id", vec!["spec_user", "other_user"]);
    let direct_messages_list_handler = handlers::RocketchatDirectMessagesList {
        direct_messages: direct_messages,
        status: status::Ok,
    };
    rocketchat_router.get(DIRECT_MESSAGES_LIST_PATH, direct_messages_list_handler, "direct_messages_list");

    let mut matrix_router = test.default_matrix_routes();
    let (forget_message_forwarder, forget_receiver) = MessageForwarder::new();
    let (leave_room, leave_receiver) = handlers::MatrixLeaveRoom::with_forwarder(test.config.as_url.clone());
    matrix_router.post(LeaveRoomEndpoint::router_path(), leave_room, "leave_room");
    matrix_router.post(ForgetRoomEndpoint::router_path(), forget_message_forwarder, "forget_room");

    let test = test.with_matrix_routes(matrix_router)
        .with_rocketchat_mock()
        .with_custom_rocketchat_routes(rocketchat_router)
        .with_connected_admin_room()
        .with_logged_in_user()
        .run();

    let direct_message = Message {
        message_id: "spec_id_1".to_string(),
        token: Some(RS_TOKEN.to_string()),
        channel_id: "spec_user_id_other_user_id".to_string(),
        channel_name: None,
        user_id: "other_user_id".to_string(),
        user_name: "other_user".to_string(),
        text: "Hey there".to_string(),
    };
    let direct_message_payload = to_string(&direct_message).unwrap();

    helpers::simulate_message_from_rocketchat(&test.config.as_url, &direct_message_payload);

    helpers::join(
        &test.config.as_url,
        RoomId::try_from("!1234_id:localhost").unwrap(),
        UserId::try_from("@spec_user:localhost").unwrap(),
    );

    helpers::leave_room(
        &test.config.as_url,
        RoomId::try_from("!1234_id:localhost").unwrap(),
        UserId::try_from("@spec_user:localhost").unwrap(),
    );

    // no calls to the leave and forget endpoints, because the virtual user stays in the room
    assert!(leave_receiver.recv_timeout(default_timeout()).is_err());
    assert!(forget_receiver.recv_timeout(default_timeout()).is_err());

    let connection = test.connection_pool.get().unwrap();
    let room = Room::find_by_rocketchat_room_id(&connection, "rc_id".to_string(), "spec_user_id_other_user_id".to_string())
        .unwrap()
        .unwrap();

    let users = room.users(&connection).unwrap();
    assert_eq!(users.len(), 1);
    assert!(users.iter().any(|u| u.matrix_user_id == UserId::try_from("@rocketchat_other_user_id_rc_id:localhost").unwrap()));

    assert!(!room.is_bridged)
}

#[test]
fn successfully_forwards_a_direct_message_to_a_room_that_was_bridged_before() {
    let test = Test::new();

    let mut rocketchat_router = Router::new();
    let mut direct_messages = HashMap::new();
    direct_messages.insert("spec_user_id_other_user_id", vec!["spec_user", "other_user"]);
    let direct_messages_list_handler = handlers::RocketchatDirectMessagesList {
        direct_messages: direct_messages,
        status: status::Ok,
    };
    rocketchat_router.get(DIRECT_MESSAGES_LIST_PATH, direct_messages_list_handler, "direct_messages_list");

    let mut matrix_router = test.default_matrix_routes();
    let (message_forwarder, receiver) = MessageForwarder::new();
    matrix_router.put(SendMessageEventEndpoint::router_path(), message_forwarder, "send_message_event");

    let test = test.with_matrix_routes(matrix_router)
        .with_rocketchat_mock()
        .with_custom_rocketchat_routes(rocketchat_router)
        .with_connected_admin_room()
        .with_logged_in_user()
        .run();

    let direct_message = Message {
        message_id: "spec_id_1".to_string(),
        token: Some(RS_TOKEN.to_string()),
        channel_id: "spec_user_id_other_user_id".to_string(),
        channel_name: None,
        user_id: "other_user_id".to_string(),
        user_name: "other_user".to_string(),
        text: "Hey there".to_string(),
    };
    let direct_message_payload = to_string(&direct_message).unwrap();

    helpers::simulate_message_from_rocketchat(&test.config.as_url, &direct_message_payload);

    // discard welcome message
    receiver.recv_timeout(default_timeout()).unwrap();
    // discard connect message
    receiver.recv_timeout(default_timeout()).unwrap();
    // discard login message
    receiver.recv_timeout(default_timeout()).unwrap();

    let message_received_by_matrix = receiver.recv_timeout(default_timeout()).unwrap();
    assert!(message_received_by_matrix.contains("Hey there"));

    helpers::join(
        &test.config.as_url,
        RoomId::try_from("!1234_id:localhost").unwrap(),
        UserId::try_from("@spec_user:localhost").unwrap(),
    );

    helpers::leave_room(
        &test.config.as_url,
        RoomId::try_from("!1234_id:localhost").unwrap(),
        UserId::try_from("@spec_user:localhost").unwrap(),
    );

    let connection = test.connection_pool.get().unwrap();
    let room = Room::find_by_rocketchat_room_id(&connection, "rc_id".to_string(), "spec_user_id_other_user_id".to_string())
        .unwrap()
        .unwrap();

    assert!(!room.is_bridged);

    let direct_message = Message {
        message_id: "spec_id_2".to_string(),
        token: Some(RS_TOKEN.to_string()),
        channel_id: "spec_user_id_other_user_id".to_string(),
        channel_name: None,
        user_id: "other_user_id".to_string(),
        user_name: "other_user".to_string(),
        text: "Hey again".to_string(),
    };
    let direct_message_payload = to_string(&direct_message).unwrap();

    helpers::simulate_message_from_rocketchat(&test.config.as_url, &direct_message_payload);

    helpers::join(
        &test.config.as_url,
        RoomId::try_from("!1234_id:localhost").unwrap(),
        UserId::try_from("@spec_user:localhost").unwrap(),
    );

    let message_received_by_matrix = receiver.recv_timeout(default_timeout()).unwrap();
    assert!(message_received_by_matrix.contains("Hey again"));
    let room = Room::find_by_rocketchat_room_id(&connection, "rc_id".to_string(), "spec_user_id_other_user_id".to_string())
        .unwrap()
        .unwrap();

    assert!(room.is_bridged);
}

#[test]
fn no_room_is_created_when_the_user_doesn_not_have_access_to_the_matching_direct_message_channel() {
    let test = Test::new();
    let (create_room_forwarder, create_room_receiver) = handlers::MatrixCreateRoom::with_forwarder(test.config.as_url.clone());
    let mut matrix_router = test.default_matrix_routes();
    matrix_router.post(CreateRoomEndpoint::router_path(), create_room_forwarder, "create_room");
    let mut rocketchat_router = Router::new();
    let direct_messages_list_handler = handlers::RocketchatDirectMessagesList {
        direct_messages: HashMap::new(),
        status: status::Ok,
    };
    rocketchat_router.get(DIRECT_MESSAGES_LIST_PATH, direct_messages_list_handler, "direct_messages_list");

    let test = test.with_matrix_routes(matrix_router)
        .with_rocketchat_mock()
        .with_custom_rocketchat_routes(rocketchat_router)
        .with_connected_admin_room()
        .with_logged_in_user()
        .run();

    let direct_message = Message {
        message_id: "spec_id_1".to_string(),
        token: Some(RS_TOKEN.to_string()),
        channel_id: "spec_user_id_other_user_id".to_string(),
        channel_name: None,
        user_id: "other_user_id".to_string(),
        user_name: "other_user".to_string(),
        text: "Hey there".to_string(),
    };
    let direct_message_payload = to_string(&direct_message).unwrap();

    helpers::simulate_message_from_rocketchat(&test.config.as_url, &direct_message_payload);

    // no room is created on the Matrix server
    create_room_receiver.recv_timeout(default_timeout()).is_err();

    let connection = test.connection_pool.get().unwrap();
    let room_option =
        Room::find_by_rocketchat_room_id(&connection, "rc_id".to_string(), "spec_user_id_other_user_id".to_string()).unwrap();
    assert!(room_option.is_none());
}

#[test]
fn no_room_is_created_when_no_matching_user_for_the_room_name_is_found() {
    let test = Test::new();
    let (create_room_forwarder, create_room_receiver) = handlers::MatrixCreateRoom::with_forwarder(test.config.as_url.clone());
    let mut matrix_router = test.default_matrix_routes();
    matrix_router.post(CreateRoomEndpoint::router_path(), create_room_forwarder, "create_room");

    let test =
        test.with_matrix_routes(matrix_router).with_rocketchat_mock().with_connected_admin_room().with_logged_in_user().run();

    let direct_message = Message {
        message_id: "spec_id_1".to_string(),
        token: Some(RS_TOKEN.to_string()),
        channel_id: "no_user_matches_this_channel_id".to_string(),
        channel_name: None,
        user_id: "other_user_id".to_string(),
        user_name: "other_user".to_string(),
        text: "Hey there".to_string(),
    };
    let direct_message_payload = to_string(&direct_message).unwrap();

    helpers::simulate_message_from_rocketchat(&test.config.as_url, &direct_message_payload);

    // no room is created on the Matrix server
    create_room_receiver.recv_timeout(default_timeout()).is_err();

    let connection = test.connection_pool.get().unwrap();
    let room_option =
        Room::find_by_rocketchat_room_id(&connection, "rc_id".to_string(), "no_user_matches_this_channel_id".to_string())
            .unwrap();
    assert!(room_option.is_none());
}

#[test]
fn no_room_is_created_when_getting_the_direct_message_list_failes() {
    let test = Test::new();
    let (create_room_forwarder, create_room_receiver) = handlers::MatrixCreateRoom::with_forwarder(test.config.as_url.clone());
    let mut matrix_router = test.default_matrix_routes();
    matrix_router.post(CreateRoomEndpoint::router_path(), create_room_forwarder, "create_room");
    let mut rocketchat_router = Router::new();
    rocketchat_router.get(
        DIRECT_MESSAGES_LIST_PATH,
        handlers::RocketchatErrorResponder {
            status: status::InternalServerError,
            message: "Getting DMs failed".to_string(),
        },
        "direct_messages_list",
    );

    let test = test.with_matrix_routes(matrix_router)
        .with_rocketchat_mock()
        .with_custom_rocketchat_routes(rocketchat_router)
        .with_connected_admin_room()
        .with_logged_in_user()
        .run();

    let direct_message = Message {
        message_id: "spec_id_1".to_string(),
        token: Some(RS_TOKEN.to_string()),
        channel_id: "spec_user_id_other_user_id".to_string(),
        channel_name: None,
        user_id: "other_user_id".to_string(),
        user_name: "other_user".to_string(),
        text: "Hey there".to_string(),
    };
    let direct_message_payload = to_string(&direct_message).unwrap();

    helpers::simulate_message_from_rocketchat(&test.config.as_url, &direct_message_payload);

    // no room is created on the Matrix server
    create_room_receiver.recv_timeout(default_timeout()).is_err();

    let connection = test.connection_pool.get().unwrap();
    let room_option =
        Room::find_by_rocketchat_room_id(&connection, "rc_id".to_string(), "spec_user_id_other_user_id".to_string()).unwrap();
    assert!(room_option.is_none());
}

#[test]
fn no_room_is_created_when_the_direct_message_list_response_cannot_be_deserialized() {
    let test = Test::new();
    let (create_room_forwarder, create_room_receiver) = handlers::MatrixCreateRoom::with_forwarder(test.config.as_url.clone());
    let mut matrix_router = test.default_matrix_routes();
    matrix_router.post(CreateRoomEndpoint::router_path(), create_room_forwarder, "create_room");
    let mut rocketchat_router = Router::new();
    rocketchat_router.get(
        DIRECT_MESSAGES_LIST_PATH,
        handlers::InvalidJsonResponse { status: status::Ok },
        "direct_messages_list",
    );

    let test = test.with_matrix_routes(matrix_router)
        .with_rocketchat_mock()
        .with_custom_rocketchat_routes(rocketchat_router)
        .with_connected_admin_room()
        .with_logged_in_user()
        .run();

    let direct_message = Message {
        message_id: "spec_id_1".to_string(),
        token: Some(RS_TOKEN.to_string()),
        channel_id: "spec_user_id_other_user_id".to_string(),
        channel_name: None,
        user_id: "other_user_id".to_string(),
        user_name: "other_user".to_string(),
        text: "Hey there".to_string(),
    };
    let direct_message_payload = to_string(&direct_message).unwrap();

    helpers::simulate_message_from_rocketchat(&test.config.as_url, &direct_message_payload);

    // no room is created on the Matrix server
    create_room_receiver.recv_timeout(default_timeout()).is_err();

    let connection = test.connection_pool.get().unwrap();
    let room_option =
        Room::find_by_rocketchat_room_id(&connection, "rc_id".to_string(), "spec_user_id_other_user_id".to_string()).unwrap();
    assert!(room_option.is_none());
}
