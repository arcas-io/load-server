# WebRTC Server

## Configuration
First, copy the example config:
```shell
cp .env.example .env
```
Update `.env` with the appropriate values.


## Running
```shell
RUST_LOG=INFO cargo run
```

## Building the Docker Image
```shell
docker build . -t "littlebearlabs/server"
```

## Running Docker
```shell
docker run -p 50051:50051 "littlebearlabs/server"
```

## API

### Create a New Session
```shell
grpcurl -plaintext -import-path ./proto -proto webrtc.proto -d '{"name": "First Session"}' [::]:50051 webrtc.WebRtc/CreateSession
```

### Starting a Session
After creating a session:

```shell
grpcurl -plaintext -import-path ./proto -proto webrtc.proto -d '{"sessionId": "9s-KsEPQkO_IgfINBV4x6"}' [::]:50051 webrtc.WebRtc/StartSession
```

### Stopping a Session
After creating and starting a session:

```shell
grpcurl -plaintext -import-path ./proto -proto webrtc.proto -d '{"sessionId": "9s-KsEPQkO_IgfINBV4x6"}' [::]:50051 webrtc.WebRtc/StopSession
```
