$(document).ready(function(){

	$("ul.md-nav__list li").each(function() {

		if ($(this).hasClass("md-nav__link--active")) {
  			$(this).find('.arrow').addClass("arrow-animate");
  			$(this).find('.submenu').slideDown(200, ani(this));
		}

		if($('.md-nav__link--active').length > 0) {
			$('.md-nav__list li:has(.md-nav__link--active)').addClass('md-nav__link--active');
			$('.md-nav__list li:has(.md-nav__link--active)').find('.arrow').addClass("arrow-animate");
			$('.md-nav__list li:has(.md-nav__link--active)').find('.submenu').slideDown(200, ani(this));
		}
	});


	$('#menu').children('ul.md-nav__list').on('click', 'li .arrow', function(e) {
	    e.preventDefault();
	    $(this).parent().find('.arrow').addClass("arrow-animate");

	    var $menu_item = $(this).closest('li');
	    var $sub_menu = $menu_item.find('.submenu');
	    var $other_sub_menus = $menu_item.siblings().find('.submenu');

	    $menu_item.addClass('selected');

	    if ($sub_menu.is(':visible')) {
	      	$sub_menu.slideUp(200, ani(this));
	      	$menu_item.removeClass('selected');
	      	$menu_item.find('.arrow').removeClass("arrow-animate");
	    } else {
	      	$other_sub_menus.slideUp(200);
	      	$sub_menu.slideDown(200, ani(this));
	      	$menu_item.siblings().removeClass('selected');
	      	$menu_item.siblings().find('.arrow').removeClass("arrow-animate");
	      	$menu_item.addClass('selected');
	      	
	    }
	});

	function ani(where) {
	  	return function() {
	    	$('body').animate({
	      		scrollTop: $(where).offset().top
	    	}, 300);
	  	}
	}


}); 