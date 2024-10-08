/**
 * @file usb/classdriver/keyboard.hpp
 *
 * HID keyboard class driver.
 */

#pragma once

#include <functional>
#include "usb/classdriver/hid.hpp"

namespace usb {
  // #@@range_begin(hid_keyboard)
  class HIDKeyboardDriver : public HIDBaseDriver {
   public:
    HIDKeyboardDriver(Device* dev, int interface_index);

    void* operator new(size_t size);
    void operator delete(void* ptr) noexcept;

    Error OnDataReceived() override;

    using ObserverType = void (uint8_t modifier, uint8_t keycode, bool press);
    void SubscribeKeyPush(std::function<ObserverType> observer);
    static std::function<ObserverType> default_observer;
    static void SetDefaultObserver(ObserverType *observer);

   private:
    std::array<std::function<ObserverType>, 4> observers_;
    int num_observers_ = 0;

    void NotifyKeyPush(uint8_t modifier, uint8_t keycode, bool press);
  };
  // #@@range_end(hid_keyboard)
}
