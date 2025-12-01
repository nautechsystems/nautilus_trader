This example shows how to send commands to a Nautilus process. 

1. Start a Redis database server
2. Start a Nautilus process by running `receiver.py`
3. Send a command by running `sender.py`
4. `on_command {'key1': 'val1'}` message should appear in Nautilus log