package tests

import (
	"app/payments/models"
	"app/payments/repository"
	"app/payments/service"
)

func test_charge_saves_payment() {
	payment := Payment{}
	repository := PaymentRepository{}
	service := PaymentService{}
	service.Charge(repository, payment)
}
