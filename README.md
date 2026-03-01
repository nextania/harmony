# Harmony

## Description
Harmony aims to provide secure, robust, and open source encrypted communication with high call quality. It is designed for individuals, communities, as well as organizations with a space and channel structure. In addition to providing a secure communication platform, developers may build upon the platform more easily than other platforms.

This repository includes the core server software as well as the desktop client, all built in Rust, making it easy to self-host using containers. You can also use the default instance. Enterprise customers will have the option to purchase support services and hosted instances. Authentication is meant to be used with the [Nextania account services](https://github.com/nextania/account). Single sign-on through OpenID Connect providers can be configured on self-hosted instances.

The Harmony desktop client is included in this repository. Mobile clients will be developed in the future. Bindings are already included through UniFFI, so mobile clients can be developed natively in Kotlin and Swift. The desktop client is built using Iced, a native Rust UI framework. The client is designed to be lightweight and performant, with a focus on providing a seamless user experience.

## Architecture
The main Harmony server is built on WebSocket-based RPC. It is designed to be horizontally scalable, with a stateless architecture. Persistence and state synchronization are handled through MongoDB and Redis. All messaging and voice traffic is end-to-end encrypted. Voice traffic is relayed through Pulse, an in-house WebTransport-based media relay. This allows for less complexity compared to a WebRTC SFU and leverages the performance benefits of WebTransport, which is also now supported by major browsers.

Harmony is not a federated service for the sake of simplicity; instead, it is a centralized service that can be self-hosted.

## Running in development
You will need Rust, MongoDB, Redis, as well as an instance of the account service to run the server.
The following environment variables need to be set:
* `MONGODB_URI` - The URI to the MongoDB database.
* `MONGODB_DATABASE` - The name of the MongoDB database.
* `REDIS_URI` - The URI to the Redis database.
* `AS_URI` - The URI of the account service. Used for authentication and introspection.
* `AS_TOKEN` - The token used for authentication with the account service. This should be kept secret and added to the account service configuration as well.

To run the server, you can use `cargo run --bin harmony`.

In addition, to run the voice node, you will need to set the following environment variables:
* `REDIS_URI` - The URI to the Redis database.
* `REGION` - The region of the voice node.
* `PUBLIC_ADDRESS` - The public address of the voice node. This should be the IP address or domain name that clients will connect to.

To run the voice node, you can use `cargo run --bin pulse`.

## Deployment
The server and voice node can be deployed easily using Docker. You can build the Docker images using the provided Dockerfiles. The server and voice node can be run using the provided `docker-compose.yml` file, which sets up the necessary services and environment variables. You can run multiple instances of both servers with minimal configuration changes to scale horizontally.

The voice node needs to be run using a server with a public IP address with the server's UDP port open to function properly. Be aware of the amount of bandwidth the voice node may use, as it can be significant with many users.

## License
<img align="right" height="100" alt="GNU AGPLv3" src="https://github.com/user-attachments/assets/4df7df05-0123-45d9-b7a9-cceb64e514d9" /> Harmony is licensed under the GNU Affero General Public License v3.0. See the [LICENSE](LICENSE) file for details.
Contributions are welcome! 
