# HTTP entrypoint for charges.

class PaymentController
  # Create a payment via the service.
  # @param service [PaymentService]
  # @param payment [Payment]
  def create(service, payment)
    service.charge(payment)
  end
end
