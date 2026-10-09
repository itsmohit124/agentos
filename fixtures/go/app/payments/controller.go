package payments

// HTTP entrypoint for charges.

type PaymentController struct{}

func (c *PaymentController) Create(service PaymentService, payment Payment) Payment {
	return service.Charge(payment)
}
