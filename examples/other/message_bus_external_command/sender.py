import msgspec
from redis import Redis


def main():
    redis = Redis('redis')
    command = {"key1": "val1"}
    redis.xadd(
        'external_stream',
        {
            'topic': 'events.command',
            'payload': msgspec.msgpack.encode(command)  # Nautilus uses Msgpack encoding by default
        }
    )


if __name__ == '__main__':
    main()
