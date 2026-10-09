# A captured charge.

PAYMENT_STATUS = "captured"

class Payment
  def initialize(payment_id, amount)
    @payment_id = payment_id
    @amount = amount
  end
end
