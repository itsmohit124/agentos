# Application service for charges.

class PaymentService
  # Charge a payment by saving it.
  # @param repository [PaymentRepository]
  # @param payment [Payment]
  def charge(repository, payment)
    repository.save(payment)
  end
end
