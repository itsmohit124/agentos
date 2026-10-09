# Billing requirements

The billing area has no code yet. Finance described the words the implementation
should use. Nothing here has been confirmed by the team.

Billing is a bounded context.

Invoice is an entity in Billing.

Money is a value object in Billing.

InvoiceAggregate is an aggregate in Billing.

Invoice has value object Money.

An invoice is rejected unless amount > 0.

## Ubiquitous language

- Invoice: a request for payment
- Money: an amount and a currency
