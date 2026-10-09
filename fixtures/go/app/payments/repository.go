package payments

// Persistence for charges.

type PaymentStore interface {
	Save(payment Payment) Payment
}

type BaseRepository struct{}

func (r *BaseRepository) Save(payment Payment) Payment {
	return payment
}

type PaymentRepository struct {
	BaseRepository
}

func (r *PaymentRepository) Save(payment Payment) Payment {
	return payment
}
