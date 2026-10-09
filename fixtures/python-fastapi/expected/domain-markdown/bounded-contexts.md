# Bounded contexts

## Confirmed and canonical

(none)

## Proposed

- **Notifications** (`domain.notifications.bounded_context.Notifications`) — Package notifications Evidence: `app/notifications/service.py` via package-heuristic, confidence 550/1000. classes: NotificationService
- **Payments** (`domain.payments.bounded_context.Payments`) — Package payments Evidence: `app/payments/models.py` via package-heuristic, confidence 800/1000. classes: Money, Payment, PaymentAggregate, PaymentRepository, PaymentService
- **Users** (`domain.users.bounded_context.Users`) — Package users Evidence: `app/users/models.py` via package-heuristic, confidence 800/1000. classes: User, UserService
