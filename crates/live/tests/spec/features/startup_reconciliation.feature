Feature: Startup reconciliation
  When startup reconciliation is enabled, included venue position reports
  must be reconciled before actors and strategies start.
  An unresolved position report blocks startup.

  Background:
    Given startup reconciliation is enabled
    And position reports are included in reconciliation
    And missing-order generation is enabled
    And the cache has an ETH/USDT perpetual instrument and margin account but no orders or positions

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
