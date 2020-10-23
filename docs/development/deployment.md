# Deployment

*** This documentation is currently a work in progress ***

## Encryption

For effective remote deployment of a live `TradingNode` which needs to access remote services, 
encryption keys must be user generated. The currently supported encryption scheme is that which is built 
into ZeroMQ being Curve25519 elliptic curve algorithms. This allows perfect forward security with 
ephemeral keys being exchanged per connection. The public `server.key` must be shared with the trader 
ahead of time and contained in the `keys\` directory (see below).

To generate a new client key pair from a python console or .py run the following;

    from pathlib import Path

    import zmq.auth

    keys_dir = 'path/to/your/keys'
    Path(keys_dir).mkdir(parents=True, exist_ok=True)

    zmq.auth.create_certificates(keys_dir, 'client')

## Live Deployment

The user must assemble a directory including the following;

- `config.json` for configuration settings
- `keys/` directory containing the `client.key_secret` and `server.key`
- `launch.py` referring to the strategies to run
- trading strategy python or cython files