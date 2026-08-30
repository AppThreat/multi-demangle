! Fortran corpus fixture.
!
! Each construct here is chosen to pin down one claim the `fortran` backend
! makes. The point is not coverage for its own sake: compile this, run `nm`,
! and the resulting symbols are ground truth that no hand-written test can
! contradict.
!
! SETTLED with this fixture: src/fortran.rs once stripped a trailing
! `_<digits>` from the procedure part, which would have silently corrupted
! every procedure whose name ends in digits. The emitted symbols below
! (`__numerics_MOD_interp_3`, `__numerics_MOD_step_12`) show gfortran
! appends no such suffix; the backend keeps the digits.

module m
  implicit none
  integer :: counter          ! module variable -> __m_MOD_counter
contains
  subroutine foo()
  end subroutine foo
  function bar(x) result(y)
    integer, intent(in) :: x
    integer :: y
    y = x
  end function bar
end module m

! A module name containing underscores, and procedure names containing
! underscores: the `_MOD_` separator must still be found unambiguously.
module my_module
  implicit none
  real :: shared_state
contains
  subroutine my_proc()
  end subroutine my_proc
  subroutine a_b_c_d()
  end subroutine a_b_c_d
end module my_module

! Procedures whose names END IN _<digits>. These are the decisive cases for
! the strip_length_suffix rule.
module numerics
  implicit none
contains
  subroutine interp_3()
  end subroutine interp_3
  subroutine solve_2d()
  end subroutine solve_2d
  subroutine step_12()
  end subroutine step_12
  subroutine plain()
  end subroutine plain
end module numerics

! A module whose name itself ends in digits, plus a very long name (any
! length-based renaming would show up here).
module grid2
  implicit none
contains
  subroutine a_very_long_procedure_name_that_goes_on_for_a_while()
  end subroutine a_very_long_procedure_name_that_goes_on_for_a_while
end module grid2

! Submodules use their own mangling; collect them to find out what it is.
module parent_mod
  implicit none
  interface
    module subroutine child_proc()
    end subroutine child_proc
  end interface
end module parent_mod

submodule (parent_mod) child_sub
contains
  module subroutine child_proc()
  end subroutine child_proc
end submodule child_sub

! Bare external subprograms: the plain g77 `name_` form that auto-detection
! deliberately never claims. `standalone` has no underscore, `two_words`
! does — the backend treats those two cases differently, so both are needed.
subroutine standalone()
end subroutine standalone

subroutine two_words()
end subroutine two_words

program corpus
  use m
  use my_module
  use numerics
  implicit none
  call foo()
  call my_proc()
  call interp_3()
end program corpus
