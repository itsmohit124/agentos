# Persistence for charges.

class BaseRepository
  def save(payment)
    payment
  end
end

class PaymentRepository < BaseRepository
  def save(payment)
    payment
  end
end
