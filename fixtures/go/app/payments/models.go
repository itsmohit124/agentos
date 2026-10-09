package payments

// A captured charge.

const PaymentStatus = "captured"

type Payment struct {
	ID     string
	Amount int
}
