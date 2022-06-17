count = 0
events = []


def increment(event):
    global count
    global events
    count += 1
    events.append(event)


def display():
    global count
    print(count)
