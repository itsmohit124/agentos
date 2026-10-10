package gosample

import "testing"

func TestAdd(t *testing.T) {
	if Add(1, 1) != 2 {
		t.Fatal("sum")
	}
}
