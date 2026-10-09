/** Charge summary shown in the payments view. */

export const PaymentBadge = (payment: Payment): string => {
  return paymentLabel(payment);
};

export function PaymentCard(payment: Payment): string {
  return <PaymentBadge payment={payment} />;
}

function paymentLabel(payment: Payment): string {
  return "captured";
}
