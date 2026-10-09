# ADR 0001: SQLAlchemy inside the order domain

## Status

Accepted

## Decision

sqlalchemy is an allowed exception inside the order domain. The legacy unit of
work is permitted to stay until the repository port is extracted.
