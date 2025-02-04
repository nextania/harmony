# Harmony
![harmony2](https://github.com/user-attachments/assets/69410aaa-837d-44bb-8208-4e5aa01eac8c)

![harmony3](https://github.com/user-attachments/assets/81b73a1c-7017-4a24-a748-a90161f55b95)


## Description
Harmony aims to provide secure, robust, and open source encrypted communication with high call quality. It is designed for individuals, communities, as well as organizations with a space and channel structure. In addition to providing a secure communication platform, developers may build upon the platform much easier than other platforms.  

This repository includes the core server software. It's free to self-host or use any hosted instance. Enterprise customers will have the option to purchase support services and hosted instances. Authentication is meant to be used with the [Nextania account services](https://github.com/nextania/account). OAuth2 will be supported in the future.

The [Harmony client](https://github.com/nextania/harmony-client) currently only exists for browsers. Other clients will be developed in the future.

Note: Harmony is not a federated service for the sake of simplicity. It is a centralized service that can be self-hosted.

## How to run
You will need Rust, MongoDB, and Redis before running the server.
The following environment variables need to be set:
* `MONGODB_URI` - The URI to the MongoDB database.
* `MONGODB_DATABASE` - The name of the MongoDB database.
* `REDIS_URI` - The URI to the Redis database.
* `JWT_SECRET` - The secret used for JWTs.
To run the server, you can use `cargo run --bin harmony`.

In addition, if you would like to run the WebRTC voice node, you will need to set the following environment variables:
* `REDIS_URI` - The URI to the Redis database.
* `REGION` - The region of the voice node.
To run the voice node, you can use `cargo run --bin pulse`.

The voice node needs to be run using a server with a public IP address for WebRTC to function properly. Additionally, a sizeable amount of bandwidth is required.

## License
This project is licensed under the [GNU Affero General Public License v3.0](https://github.com/nextania/harmony/blob/main/LICENSE).
