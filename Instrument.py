class Instrument:
    """
    A class to represent a trading instrument with exchange-specific quantity limits.
    
    Attributes:
        max_quantity_limit (int or None): Maximum quantity allowed for limit orders.
        min_quantity_limit (int or None): Minimum quantity allowed for limit orders.
        max_quantity_market (int or None): Maximum quantity allowed for market orders.
        min_quantity_market (int or None): Minimum quantity allowed for market orders.
    """
    
    def __init__(self):
        """
        Initializes an instance of the Instrument class with None values for
        all quantity limits. These will be updated based on the exchange's rules.
        """
        self.max_quantity_limit = None  # Maximum quantity for limit orders
        self.min_quantity_limit = None  # Minimum quantity for limit orders
        self.max_quantity_market = None  # Maximum quantity for market orders
        self.min_quantity_market = None  # Minimum quantity for market orders

    def set_limit_order_quantities(self, max_limit, min_limit):
        """
        Set the maximum and minimum quantities allowed for limit orders.

        Args:
            max_limit (int): The maximum quantity allowed for limit orders.
            min_limit (int): The minimum quantity allowed for limit orders.
        """
        self.max_quantity_limit = max_limit
        self.min_quantity_limit = min_limit

    def set_market_order_quantities(self, max_market, min_market):
        """
        Set the maximum and minimum quantities allowed for market orders.

        Args:
            max_market (int): The maximum quantity allowed for market orders.
            min_market (int): The minimum quantity allowed for market orders.
        """
        self.max_quantity_market = max_market
        self.min_quantity_market = min_market

    def __repr__(self):
        """
        Return a string representation of the Instrument object.
        This is helpful for debugging and understanding the state of the object.

        Returns:
            str: A string that represents the Instrument object with its quantity limits.
        """
        return (f"Instrument(max_quantity_limit={self.max_quantity_limit}, "
                f"min_quantity_limit={self.min_quantity_limit}, "
                f"max_quantity_market={self.max_quantity_market}, "
                f"min_quantity_market={self.min_quantity_market})")

    def validate_order(self, order_type, quantity):
        """
        Validates the order quantity based on the order type and the exchange rules.

        Args:
            order_type (str): The type of order ('limit' or 'market').
            quantity (int): The quantity of the order to be validated.

        Returns:
            bool: True if the order is valid based on the rules, False otherwise.
        """
        if order_type == 'limit':
            return (self.min_quantity_limit <= quantity <= self.max_quantity_limit)
        elif order_type == 'market':
            return (self.min_quantity_market <= quantity <= self.max_quantity_market)
        else:
            raise ValueError("Invalid order type. Use 'limit' or 'market'.")
