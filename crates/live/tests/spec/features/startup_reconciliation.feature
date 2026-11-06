Feature: Startup reconciliation
  When startup reconciliation is enabled, included venue position reports
  must be reconciled before actors and strategies start.
  An unresolved position report blocks startup.

  Background:
    Given startup reconciliation is enabled
    And position reports are included in reconciliation
    And missing-order generation is enabled
    And the cache has an ETH/USDT perpetual instrument and margin account but no orders or positions

  Rule: NETTING startup exposes reconciled state before strategies start

    @netting @flat
    Scenario Outline: Start flat with an empty cache using <entrypoint>
      Given the execution client uses NETTING accounting
      And the venue reports no orders, fills, or positions
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy observes no open orders or positions when it starts
      And the portfolio exposes 0 ETH when the strategy starts
      And no position is cached
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @netting @recovery
    Scenario Outline: Recover a reported NETTING <side> position with an empty cache using <entrypoint>
      Given the execution client uses NETTING accounting
      And the venue reports a <side> position of 5.000 ETH
      And the position report has no venue position ID
      And its average opening price is 2500.25 USDT
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy observes a <side> position of 5.000 ETH at 2500.25 USDT when it starts
      And the portfolio exposes <signed_qty> ETH when the strategy starts
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | side  | entrypoint | signed_qty |
        | long  | start      | 5.000      |
        | long  | run        | 5.000      |
        | short | start      | -5.000     |
        | short | run        | -5.000     |

  Rule: A late report after restart must not count a recovered execution twice
    A startup position snapshot can already include an execution whose Filled
    order report arrives later. Even after the node is running, recovering that
    order must leave the recovered position and portfolio exposure unchanged.

    @netting @recovery @late_report
    Scenario Outline: Receive an earlier execution report after recovering a <side> position at restart
      Given the execution client uses NETTING accounting
      And the venue reports a <side> position of 5.000 ETH
      And the position report has no venue position ID
      And its average opening price is 2500.25 USDT
      And the execution in the delayed Filled order report is already included in the position snapshot
      When the live node restarts and receives the delayed Filled order report after entering Running
      Then startup succeeds
      And the strategy observes a <side> position of 5.000 ETH at 2500.25 USDT when it starts
      And the portfolio exposes <signed_qty> ETH when the strategy starts
      And the delayed order is FILLED for 5.000 ETH at 2500.25 USDT
      And the recovered position and portfolio exposure stay unchanged after the delayed report
      And exactly one position is cached
      And the actor and strategy each start once

      Examples:
        | side  | signed_qty |
        | long  | 5.000      |
        | short | -5.000     |

  Rule: Included position reports must be reconciled before actors and strategies start

    @recovery
    Scenario Outline: Reconcile a reported <side> position using <entrypoint>
      Given the venue reports a <side> position of 5.000 ETH
      And its average opening price is 2500.25 USDT
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the strategy observes a <side> position of 5.000 ETH at 2500.25 USDT when it starts
      And the portfolio exposes <signed_qty> ETH when the strategy starts
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | side  | entrypoint | signed_qty |
        | long  | start      | 5.000      |
        | long  | run        | 5.000      |
        | short | start      | -5.000     |
        | short | run        | -5.000     |

    @unrecoverable
    Scenario Outline: Block startup when the opening price is missing using <entrypoint>
      Given the venue reports a long position of 5.000 ETH
      And its average opening price is unavailable
      When the live node starts using <entrypoint>
      Then startup fails because the opening price is missing
      And no actor or strategy starts
      And no position is cached
      And the venue receives a startup mass-status request
      And the execution client is disconnected

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @generation_disabled
    Scenario Outline: Block startup when missing-order generation is disabled using <entrypoint>
      Given the venue reports a long position of 5.000 ETH
      And its average opening price is 2500.25 USDT
      And missing-order generation is disabled
      When the live node starts using <entrypoint>
      Then startup fails because missing-order generation is disabled
      And no actor or strategy starts
      And no position is cached
      And the venue receives a startup mass-status request
      And the execution client is disconnected

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: Startup continues when mass status is unavailable
    Unavailable mass status does not establish that the venue is flat.

    @unavailable
    Scenario Outline: Continue when the venue cannot provide mass status using <entrypoint>
      Given the venue cannot provide mass status
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the actor and strategy each start once
      And the venue receives a startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: A failed or timed-out mass-status request blocks startup

    @request_error
    Scenario Outline: Block startup when the mass-status request fails using <entrypoint>
      Given the venue returns an error for mass-status requests
      When the live node starts using <entrypoint>
      Then startup fails because the mass-status request failed
      And no actor or strategy starts
      And the venue receives a startup mass-status request
      And the execution client is disconnected

      Examples:
        | entrypoint |
        | start      |
        | run        |

    @timeout
    Scenario Outline: Block startup when the reconciliation deadline expires using <entrypoint>
      Given the venue never completes the mass-status request
      When the live node starts using <entrypoint>
      Then startup fails because the reconciliation deadline expires
      And no actor or strategy starts
      And the venue receives a startup mass-status request
      And the execution client is disconnected

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: Disabling startup reconciliation skips venue requests

    @disabled
    Scenario Outline: Skip venue requests when startup reconciliation is disabled using <entrypoint>
      Given startup reconciliation is disabled
      And the venue returns an error for mass-status requests
      When the live node starts using <entrypoint>
      Then startup succeeds
      And the actor and strategy each start once
      And the venue receives no startup mass-status request

      Examples:
        | entrypoint |
        | start      |
        | run        |

  Rule: Closing executions recover retained exposure when the venue cannot report positions
    A close executed while the node is offline must update the retained position,
    its realized profit, and its fees before the strategy starts.
    A cached order already belongs to the strategy. An externally placed order
    must be explicitly claimed by that strategy.
    A missing position report does not by itself prove that the position is flat.

    @retained_close
    Scenario Outline: Recover an offline close from a <source> order
      Given the execution client uses NETTING accounting
      And a retained long position holds 1.000 ETH bought at 3000.00 USDT with a 0.60 USDT fee
      And the client cannot provide complete position coverage
      And the closing order is <source>
      And the bounded history starts after the opening trade
      And the venue reports a filled closing order and no position
      And the venue reports these closing executions
        | quantity | price   | fee  |
        | 0.400    | 3100.00 | 0.25 |
        | 0.600    | 3120.00 | 0.37 |
      When the live node starts using start
      Then startup succeeds
      And the closing order is FILLED
      And the retained position is flat with realized profit of 110.78 USDT and total fees of 1.22 USDT when the strategy starts
      And exactly one position is cached
      And the portfolio exposes 0 ETH when the strategy starts
      And the actor and strategy each start once

      Examples:
        | source  |
        | cached  |
        | claimed |

    @retained_close @incomplete
    Scenario: Incomplete history cannot establish a safe position reduction
      Given the execution client uses NETTING accounting
      And a retained long position holds 1.000 ETH bought at 3000.00 USDT with a 0.60 USDT fee
      And the client cannot provide complete position coverage
      And the closing order is cached
      And the bounded history starts after the opening trade
      And the venue reports a filled closing order and no position
      And the venue reports these closing executions
        | quantity | price   | fee  |
        | 0.400    | 3100.00 | 0.25 |
        | 0.600    | 3120.00 | 0.37 |
      But the venue marks the history incomplete
      When the live node starts using start
      Then startup succeeds
      And the closing order is FILLED
      And the retained position remains long 1.000 ETH with realized profit of -0.60 USDT and total fees of 0.60 USDT when the strategy starts
      And exactly one position is cached
      And the portfolio exposes 1.000 ETH when the strategy starts

    @retained_close @no_executions
    Scenario: A filled order report alone does not recover closing profit
      Given the execution client uses NETTING accounting
      And a retained long position holds 1.000 ETH bought at 3000.00 USDT with a 0.60 USDT fee
      And the client cannot provide complete position coverage
      And the closing order is cached
      And the bounded history starts after the opening trade
      And the venue reports a filled closing order and no position
      But the venue supplies no closing executions
      When the live node starts using start
      Then startup succeeds
      And the closing order is FILLED
      And the retained position remains long 1.000 ETH with realized profit of -0.60 USDT and total fees of 0.60 USDT when the strategy starts
      And exactly one position is cached
      And the portfolio exposes 1.000 ETH when the strategy starts
