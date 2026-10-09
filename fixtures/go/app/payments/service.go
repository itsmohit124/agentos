package payments

// Application service for charges.

type PaymentService struct{}

func (s *PaymentService) Charge(repository PaymentRepository, payment Payment) Payment {
	return repository.Save(payment)
}
